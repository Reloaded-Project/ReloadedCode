//! Vendor-history projection and default compaction for the compact
//! chain.
//!
//! # What lives here
//!
//! - Projection: `Vec<ModelRequest>` parts become
//!   [`CompactMessage`] entries, each carrying its native part as the
//!   preserved payload; a chain-returned history rebuilds into
//!   requests, reusing untouched parts and rebuilding mutated ones
//!   from the view.
//! - Default compaction: the [`CompactExecutor`] the chain ends in.
//!   It summarizes the older entries through the run's model, keeps
//!   a recent window verbatim, and memoizes summaries on the
//!   summarized prefix: an unchanged prefix costs no request, a
//!   small grown tail stays verbatim without one, and a tail past
//!   the re-summary budget folds into the summary with one request.
//!
//! [`CompactMessage`]: reloaded_code_core::hooks::CompactMessage
//!
//! Next: see the parent module for detection, publication, and the
//! model wrapper driving this.

use RunMessageRole::{Assistant, System, Tool, User};
use reloaded_code_core::ToolError;
use reloaded_code_core::hooks::{
    CompactExecutor, CompactHookFuture, CompactMessage, CompactOutcome, CompactResult,
    HookRunContext, RunMessageRole,
};
use serdes_ai::core::messages::{RetryContent, ToolCallArgs, ToolReturnContent};
use serdes_ai::core::{
    ModelRequest, ModelRequestPart, ModelResponse, ModelResponsePart, ModelSettings,
    SystemPromptPart, UserContent, UserPromptPart,
};
use serdes_ai_models::{BoxedModel, ModelRequestParameters};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem;
use std::sync::Mutex;

/// Most-recent non-system entries the default compaction keeps
/// verbatim.
const KEEP_RECENT_ENTRIES: usize = 4;
/// New summarizable entries that accumulate before the cached
/// summary is refreshed with one model request; below it, the new
/// entries stay verbatim after the summary, so a grown prefix costs
/// no request.
const RE_SUMMARY_TAIL_ENTRIES: usize = 4;
/// Strategy name the default compaction reports.
const STRATEGY_SUMMARIZE: &str = "summarize";
/// Label introducing the summary inside the compacted history.
const SUMMARY_PREFIX: &str = "Summary of the earlier conversation:";

/// Default compaction (`original`): summarize the older entries
/// through the run's model, keep a recent window verbatim.
pub(super) struct DefaultCompactor<'a> {
    model: &'a BoxedModel,
    settings: &'a ModelSettings,
    /// Request-level token estimate of the uncompacted history.
    tokens_before: usize,
    summaries: &'a Mutex<Option<CachedSummary>>,
}

/// How the next summary reuses the memoized one.
enum SummarizePlan {
    /// Same prefix: reuse without a model request.
    Reuse { summary: String },
    /// Grown prefix under the tail budget: serve the cached summary
    /// and keep the new entries verbatim.
    Tail { summary: String, since: usize },
    /// Grown prefix at or over the tail budget: summarize the cached
    /// summary plus the new tail.
    Incremental { previous: String, since: usize },
    /// Different prefix: summarize everything.
    Full,
}

/// Memoized summary covering one summarized prefix.
pub(super) struct CachedSummary {
    /// Fingerprints of the summarized entries.
    fingerprints: Vec<u64>,
    /// Summary text covering those entries.
    summary: String,
}

impl<'a> DefaultCompactor<'a> {
    /// Assembles the default compaction over the run's model.
    pub(super) fn new(
        model: &'a BoxedModel,
        settings: &'a ModelSettings,
        tokens_before: usize,
        summaries: &'a Mutex<Option<CachedSummary>>,
    ) -> Self {
        Self {
            model,
            settings,
            tokens_before,
            summaries,
        }
    }
}

impl DefaultCompactor<'_> {
    /// Summarizes `entries`, memoized on the entry fingerprints.
    ///
    /// Returns the summary text plus the entries it does not cover:
    /// a grown prefix under the tail budget keeps its new entries
    /// verbatim instead of paying a model request.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the summarization request fails or
    /// returns no text.
    async fn summarize(
        &self,
        entries: Vec<CompactMessage>,
    ) -> Result<(String, Vec<CompactMessage>), ToolError> {
        let fingerprints: Vec<u64> = entries.iter().map(entry_fingerprint).collect();
        // The lock only picks the plan; the request runs unlocked.
        let plan = {
            let mut cached = self
                .summaries
                .lock()
                .expect("summary cache should not be poisoned");
            match cached.as_mut() {
                Some(cached) if cached.fingerprints == fingerprints => SummarizePlan::Reuse {
                    summary: cached.summary.clone(),
                },
                Some(cached)
                    if fingerprints.len() > cached.fingerprints.len()
                        && cached.fingerprints == fingerprints[..cached.fingerprints.len()] =>
                {
                    let aged = fingerprints.len() - cached.fingerprints.len();
                    if aged < RE_SUMMARY_TAIL_ENTRIES {
                        SummarizePlan::Tail {
                            summary: cached.summary.clone(),
                            since: cached.fingerprints.len(),
                        }
                    } else {
                        SummarizePlan::Incremental {
                            previous: cached.summary.clone(),
                            since: cached.fingerprints.len(),
                        }
                    }
                }
                _ => SummarizePlan::Full,
            }
        };
        let mut entries = entries;
        let (summary, tail) = match plan {
            SummarizePlan::Reuse { summary } => (summary, Vec::new()),
            SummarizePlan::Tail { summary, since } => {
                let tail = entries.split_off(since);
                // The cache still describes the summarized prefix;
                // leave it for the next, larger attempt.
                return Ok((summary, tail));
            }
            SummarizePlan::Incremental { previous, since } => {
                let delta = format!(
                    "Summary so far:\n{previous}\n\nMessages since:\n{}",
                    transcript(&entries[since..]),
                );
                (self.request_summary(delta).await?, Vec::new())
            }
            SummarizePlan::Full => (
                self.request_summary(transcript(&entries)).await?,
                Vec::new(),
            ),
        };
        *self
            .summaries
            .lock()
            .expect("summary cache should not be poisoned") = Some(CachedSummary {
            fingerprints,
            summary: summary.clone(),
        });
        Ok((summary, tail))
    }

    /// Requests one summary from the run's model.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the request fails or the response
    /// carries no text.
    async fn request_summary(&self, transcript: String) -> Result<String, ToolError> {
        let request = ModelRequest::with_parts(vec![
            ModelRequestPart::SystemPrompt(SystemPromptPart::new(super::SUMMARY_SYSTEM_PROMPT)),
            ModelRequestPart::UserPrompt(UserPromptPart::new(transcript)),
        ]);
        let response = self
            .model
            .request(
                &[request],
                self.settings,
                &ModelRequestParameters::default(),
            )
            .await
            .map_err(|error| {
                ToolError::Execution(format!(
                    "context compaction summary request failed: {error}"
                ))
            })?;
        collect_summary_text(&response)
    }
}

impl CompactExecutor for DefaultCompactor<'_> {
    fn execute<'a>(
        &'a self,
        _ctx: &'a HookRunContext<'a>,
        mut history: Vec<CompactMessage>,
    ) -> CompactHookFuture<'a> {
        Box::pin(async move {
            let messages_before = history.len();
            let Some(kept_start) = kept_window_start(&history) else {
                // Nothing summarizable: apply no history change.
                return Ok((CompactOutcome::Cancelled, history));
            };
            let leading_systems = leading_system_count(&history);
            let kept = history.split_off(kept_start);
            // The summarize side moves out by value: entry slices are
            // not shareable across await points.
            let older = history.split_off(leading_systems);
            // Summarization failure aborts the attempt; the caller
            // serves the original history.
            let (summary, tail) = self.summarize(older).await?;
            history.push(CompactMessage::new(
                RunMessageRole::System,
                format!("{SUMMARY_PREFIX}\n{summary}"),
            ));
            history.extend(tail);
            history.extend(kept);
            let tokens_after = estimate_entry_tokens(&history);
            let result = CompactResult {
                summary,
                first_kept_entry_id: None,
                tokens_before: self.tokens_before,
                tokens_after,
                strategy: STRATEGY_SUMMARIZE.to_owned(),
                messages_before,
                messages_after: history.len(),
            };
            Ok((CompactOutcome::Compacted(result), history))
        })
    }
}

/// Estimated tokens of one model request, mirroring the vendor's
/// `ContextInfo` heuristic: serialized message and tool bytes over
/// four. `tools_bytes` is the caller-cached serialized length of the
/// request's tool definitions.
pub(super) fn estimate_request_tokens(messages: &[ModelRequest], tools_bytes: usize) -> usize {
    let messages_bytes = serde_json::to_string(messages).map_or(0, |json| json.len());
    (messages_bytes + tools_bytes) / 4
}

/// Index where the kept window starts, or `None` while the history
/// is too short to compact.
///
/// Leading system entries are never summarized; the window covers
/// the last [`KEEP_RECENT_ENTRIES`] entries after them. A tool entry
/// at the window start answers a call that would be summarized, so
/// the start moves past such entries.
pub(super) fn kept_window_start(entries: &[CompactMessage]) -> Option<usize> {
    let leading = leading_system_count(entries);
    if entries.len() - leading <= KEEP_RECENT_ENTRIES {
        return None;
    }
    let mut start = entries.len() - KEEP_RECENT_ENTRIES;
    while start < entries.len() && entries[start].role() == RunMessageRole::Tool {
        start += 1;
    }
    (start < entries.len()).then_some(start)
}

/// Projects the vendor history onto compact-chain entries, carrying
/// each native part as the preserved payload.
pub(super) fn project_history(messages: &[ModelRequest]) -> Vec<CompactMessage> {
    let part_count = messages.iter().map(|message| message.parts.len()).sum();
    let mut entries = Vec::with_capacity(part_count);
    for message in messages {
        for part in &message.parts {
            let (role, text) = part_role_text(part);
            entries.push(CompactMessage::new_preserved(
                role,
                text,
                Box::new(part.clone()),
            ));
        }
    }
    entries
}

/// Applies a chain-returned history: preserved entries reuse their
/// native parts, mutated or injected entries rebuild from the view.
pub(super) fn rebuild_history(entries: Vec<CompactMessage>) -> Vec<ModelRequest> {
    entries
        .into_iter()
        .map(|mut entry| {
            let part = match entry.take_preserved() {
                Some(preserved) => match preserved.downcast::<ModelRequestPart>() {
                    Ok(part) => *part,
                    Err(_) => rebuilt_part(entry.role(), entry.text()),
                },
                None => rebuilt_part(entry.role(), entry.text()),
            };
            ModelRequest::with_parts(vec![part])
        })
        .collect()
}

/// Extracts the summary text from a summarization response.
///
/// # Errors
/// Returns [`ToolError`] when the response carries no text.
fn collect_summary_text(response: &ModelResponse) -> Result<String, ToolError> {
    let text_len: usize = response
        .parts
        .iter()
        .map(|part| match part {
            ModelResponsePart::Text(text) => text.content.len(),
            _ => 0,
        })
        .sum();
    let mut summary = String::with_capacity(text_len);
    for part in &response.parts {
        if let ModelResponsePart::Text(text) = part {
            summary.push_str(&text.content);
        }
    }
    if summary.trim().is_empty() {
        return Err(ToolError::Execution(
            "context compaction summary was empty".into(),
        ));
    }
    Ok(summary)
}

/// Stable fingerprint of one entry's summary-relevant content.
fn entry_fingerprint(entry: &CompactMessage) -> u64 {
    let mut hasher = DefaultHasher::new();
    mem::discriminant(&entry.role()).hash(&mut hasher);
    entry.text().hash(&mut hasher);
    entry.has_preserved().hash(&mut hasher);
    hasher.finish()
}

/// Estimated tokens of an entry view: text bytes over four.
fn estimate_entry_tokens(entries: &[CompactMessage]) -> usize {
    entries
        .iter()
        .map(|entry| entry.text().len())
        .sum::<usize>()
        / 4
}

/// Number of leading system entries.
fn leading_system_count(entries: &[CompactMessage]) -> usize {
    entries
        .iter()
        .take_while(|entry| entry.role() == RunMessageRole::System)
        .count()
}

/// Role and hook-visible text of one history part.
fn part_role_text(part: &ModelRequestPart) -> (RunMessageRole, String) {
    match part {
        ModelRequestPart::SystemPrompt(part) => (RunMessageRole::System, part.content.clone()),
        ModelRequestPart::UserPrompt(part) => (
            RunMessageRole::User,
            crate::agent_runtime::stream_events::user_content_text(part.content.clone()),
        ),
        ModelRequestPart::RetryPrompt(part) => (
            RunMessageRole::User,
            match &part.content {
                RetryContent::Text(text) => text.clone(),
                other => other.message().to_owned(),
            },
        ),
        ModelRequestPart::ToolReturn(part) => (
            RunMessageRole::Tool,
            match &part.content {
                ToolReturnContent::Text { content } => content.clone(),
                other => other.to_string_content(),
            },
        ),
        ModelRequestPart::BuiltinToolReturn(part) => (
            RunMessageRole::Tool,
            serde_json::to_string(&part.content).unwrap_or_else(|_| format!("{part:?}")),
        ),
        ModelRequestPart::ModelResponse(response) => {
            (RunMessageRole::Assistant, response_text(response))
        }
    }
}

/// Rebuilds a native part from the structured view.
///
/// A mutated tool entry rebuilds as a user-role note: the view
/// carries no call id, and an id-less tool result could be rejected
/// by the provider.
fn rebuilt_part(role: RunMessageRole, text: &str) -> ModelRequestPart {
    match role {
        RunMessageRole::System => ModelRequestPart::SystemPrompt(SystemPromptPart::new(text)),
        RunMessageRole::User => {
            ModelRequestPart::UserPrompt(UserPromptPart::new(UserContent::Text(text.to_owned())))
        }
        RunMessageRole::Assistant => {
            ModelRequestPart::ModelResponse(Box::new(ModelResponse::text(text)))
        }
        RunMessageRole::Tool => ModelRequestPart::UserPrompt(UserPromptPart::new(
            UserContent::Text(format!("[tool result] {text}")),
        )),
    }
}

/// Renders entries as the transcript given to the summarizer.
fn transcript(entries: &[CompactMessage]) -> String {
    // Longest role tag: brackets, name, space, and newline.
    const ROLE_TAG_LEN: usize = "[Assistant] \n".len();
    let text_len: usize = entries.iter().map(|entry| entry.text().len()).sum();
    let mut rendered = String::with_capacity(text_len + entries.len() * ROLE_TAG_LEN);
    for entry in entries {
        let role = match entry.role() {
            System => "System",
            User => "User",
            Assistant => "Assistant",
            Tool => "Tool",
        };
        rendered.push('[');
        rendered.push_str(role);
        rendered.push_str("] ");
        rendered.push_str(entry.text());
        rendered.push('\n');
    }
    rendered
}

/// Hook-visible text of one model response: text parts joined, with
/// tool calls rendered inline so call-only turns stay visible.
fn response_text(response: &ModelResponse) -> String {
    let mut text_len = 0usize;
    let mut tool_calls = 0usize;
    for part in &response.parts {
        match part {
            ModelResponsePart::Text(text) => text_len += text.content.len(),
            ModelResponsePart::ToolCall(_) => tool_calls += 1,
            _ => {}
        }
    }
    // Per tool call: newline, name, arguments, and brackets.
    let mut text = String::with_capacity(text_len + tool_calls * 64);
    for part in &response.parts {
        match part {
            ModelResponsePart::Text(text_part) => text.push_str(&text_part.content),
            ModelResponsePart::ToolCall(call) => {
                let args = match &call.args {
                    ToolCallArgs::String(raw) => raw.clone(),
                    other => other.to_json_string().unwrap_or_default(),
                };
                text.push_str(&format!("\n[tool call {}({args})]", call.tool_name));
            }
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::FunctionModel;
    use serde_json::json;
    use serdes_ai::core::ToolReturnPart;
    use std::sync::Arc;

    /// Projects one native part of the given kind into an entry.
    fn entry(role: RunMessageRole, text: &str) -> CompactMessage {
        let part = match role {
            RunMessageRole::System => ModelRequestPart::SystemPrompt(SystemPromptPart::new(text)),
            RunMessageRole::User => {
                ModelRequestPart::UserPrompt(UserPromptPart::new(UserContent::Text(text.into())))
            }
            RunMessageRole::Assistant => {
                ModelRequestPart::ModelResponse(Box::new(ModelResponse::text(text)))
            }
            RunMessageRole::Tool => {
                ModelRequestPart::ToolReturn(ToolReturnPart::success("ping", text))
            }
        };
        let (projected_role, projected_text) = part_role_text(&part);
        CompactMessage::new_preserved(projected_role, projected_text, Box::new(part))
    }

    fn run_ctx() -> HookRunContext<'static> {
        HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        }
    }

    /// History long enough to compact: one system entry, two older
    /// turns to summarize, then a four-entry kept window.
    fn long_history() -> Vec<CompactMessage> {
        vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "old question"),
            entry(RunMessageRole::Assistant, "old answer"),
            entry(RunMessageRole::User, "second old question"),
            entry(RunMessageRole::Assistant, "second old answer"),
            entry(RunMessageRole::User, "new question"),
            entry(RunMessageRole::Tool, "new tool result"),
        ]
    }

    #[test]
    fn kept_window_start_covers_recent_entries_after_leading_systems() {
        let history = long_history();
        let start = kept_window_start(&history).expect("history is long enough");
        assert_eq!(start, 3, "the kept window keeps the last four entries");
        assert_eq!(leading_system_count(&history), 1);
        assert_ne!(
            history[start].role(),
            RunMessageRole::Tool,
            "the window never starts on a tool entry"
        );
    }

    #[test]
    fn kept_window_start_folds_tool_entries_into_the_summarized_side() {
        // The window would start on a tool entry; the start moves
        // past both tool entries so their assistant call is not
        // split off.
        let history = vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "u1"),
            entry(RunMessageRole::Assistant, "a1"),
            entry(RunMessageRole::Tool, "t1"),
            entry(RunMessageRole::Tool, "t2"),
            entry(RunMessageRole::User, "u2"),
            entry(RunMessageRole::Assistant, "a2"),
        ];
        let start = kept_window_start(&history).expect("history is long enough");
        assert_eq!(start, 5, "a tool entry never starts the kept window");
    }

    #[test]
    fn kept_window_start_rejects_short_and_all_tool_histories() {
        let short = vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "u1"),
        ];
        assert_eq!(kept_window_start(&short), None);
        // Tool entries fill the window, so every candidate start
        // folds and no valid window remains.
        let all_tool = vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "u1"),
            entry(RunMessageRole::Tool, "t1"),
            entry(RunMessageRole::Tool, "t2"),
            entry(RunMessageRole::Tool, "t3"),
        ];
        assert_eq!(kept_window_start(&all_tool), None);
    }

    #[test]
    fn project_history_covers_every_part_kind_with_its_role() {
        let request = ModelRequest::with_parts(vec![
            ModelRequestPart::SystemPrompt(SystemPromptPart::new("sys")),
            ModelRequestPart::UserPrompt(UserPromptPart::new("hello")),
            ModelRequestPart::ToolReturn(ToolReturnPart::success("ping", "pong")),
            ModelRequestPart::ModelResponse(Box::new(ModelResponse::text("hi there"))),
        ]);
        let entries = project_history(&[request]);
        let roles: Vec<_> = entries.iter().map(CompactMessage::role).collect();
        assert_eq!(
            roles,
            vec![
                RunMessageRole::System,
                RunMessageRole::User,
                RunMessageRole::Tool,
                RunMessageRole::Assistant,
            ]
        );
        assert_eq!(entries[2].text(), "pong");
        assert!(entries.iter().all(CompactMessage::has_preserved));
    }

    #[test]
    fn response_text_renders_tool_calls_inline() {
        let response = ModelResponse::with_parts(vec![
            ModelResponsePart::text("checking"),
            ModelResponsePart::tool_call("ping", json!({"target": "example.com"})),
        ]);
        let text = response_text(&response);
        assert_eq!(
            text, "checking\n[tool call ping({\"target\":\"example.com\"})]",
            "call-only turns must stay visible to hooks and the summarizer"
        );
    }

    #[test]
    fn rebuild_history_reuses_preserved_parts_and_rebuilds_mutated_ones() {
        let mut history = vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "original body"),
        ];
        history[1].set_text("rewritten body");
        let rebuilt = rebuild_history(history);
        assert_eq!(rebuilt.len(), 2);
        assert!(matches!(
            &rebuilt[0].parts[0],
            ModelRequestPart::SystemPrompt(part) if part.content == "sys"
        ));
        // The rewritten entry rebuilt from the view; the untouched
        // one kept its native part.
        assert!(matches!(
            &rebuilt[1].parts[0],
            ModelRequestPart::UserPrompt(part)
                if part.content == UserContent::Text("rewritten body".into())
        ));
    }

    /// Summarizing model for default-compaction tests: records each
    /// request's user text and answers "folded".
    fn summarizing_model() -> (BoxedModel, Arc<Mutex<Vec<String>>>) {
        let transcripts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&transcripts);
        let model = FunctionModel::new(move |messages, _settings| {
            let transcript = messages
                .iter()
                .flat_map(|message| message.user_prompts())
                .map(|part| part.content.as_text().unwrap_or_default().to_owned())
                .collect::<String>();
            seen.lock()
                .expect("transcripts should not be poisoned")
                .push(transcript);
            ModelResponse::text("folded")
        });
        (Arc::new(model), transcripts)
    }

    fn compactor<'a>(
        model: &'a BoxedModel,
        settings: &'a ModelSettings,
        summaries: &'a Mutex<Option<CachedSummary>>,
    ) -> DefaultCompactor<'a> {
        DefaultCompactor {
            model,
            settings,
            tokens_before: 100,
            summaries,
        }
    }

    #[tokio::test]
    async fn default_compactor_summarizes_old_entries_and_keeps_the_window() {
        let (model, transcripts) = summarizing_model();
        let settings = ModelSettings::default();
        let summaries = Mutex::new(None);
        let (outcome, history) = compactor(&model, &settings, &summaries)
            .execute(&run_ctx(), long_history())
            .await
            .expect("default compaction should succeed");

        let CompactOutcome::Compacted(result) = outcome else {
            panic!("compaction should have run, got {outcome:?}");
        };
        assert_eq!(result.strategy, "summarize");
        assert_eq!(result.messages_before, 7);
        assert_eq!(
            result.messages_after, 6,
            "two summarized entries collapse into one summary entry"
        );
        assert_eq!(result.tokens_before, 100);
        assert!(
            result.tokens_after < 100,
            "the compacted view must estimate smaller"
        );
        // History: leading system, summary entry, kept window
        // verbatim.
        let texts: Vec<&str> = history.iter().map(CompactMessage::text).collect();
        assert_eq!(
            texts,
            vec![
                "sys",
                "Summary of the earlier conversation:\nfolded",
                "second old question",
                "second old answer",
                "new question",
                "new tool result",
            ]
        );
        assert_eq!(
            transcripts.lock().expect("transcripts not poisoned").len(),
            1,
            "one summarization request should have been made"
        );
    }

    #[tokio::test]
    async fn default_compactor_reuses_summary_for_unchanged_prefix() {
        let (model, transcripts) = summarizing_model();
        let settings = ModelSettings::default();
        let summaries = Mutex::new(None);
        let compactor = compactor(&model, &settings, &summaries);

        compactor
            .execute(&run_ctx(), long_history())
            .await
            .expect("first compaction should succeed");
        // Second attempt over the same entries: no new request.
        compactor
            .execute(&run_ctx(), long_history())
            .await
            .expect("second compaction should succeed");
        assert_eq!(
            transcripts.lock().expect("transcripts not poisoned").len(),
            1,
            "an unchanged prefix must reuse the cached summary"
        );
    }

    #[tokio::test]
    async fn default_compactor_keeps_small_grown_tail_verbatim_without_new_request() {
        let (model, transcripts) = summarizing_model();
        let settings = ModelSettings::default();
        let summaries = Mutex::new(None);
        let compactor = compactor(&model, &settings, &summaries);

        let (outcome, _) = compactor
            .execute(&run_ctx(), long_history())
            .await
            .expect("first compaction should succeed");
        let CompactOutcome::Compacted(first) = outcome else {
            unreachable!("checked above");
        };
        // A new turn slides the kept window forward by one entry:
        // under the tail budget, no model request runs and the aged
        // entry stays verbatim after the cached summary.
        let mut grown = long_history();
        grown.push(entry(RunMessageRole::User, "tail question"));
        let (outcome, history) = compactor
            .execute(&run_ctx(), grown)
            .await
            .expect("second compaction should succeed");
        let CompactOutcome::Compacted(second) = outcome else {
            unreachable!("checked above");
        };

        assert_eq!(
            transcripts.lock().expect("transcripts not poisoned").len(),
            1,
            "a small grown tail must not pay a summarization request"
        );
        let texts: Vec<&str> = history.iter().map(CompactMessage::text).collect();
        assert_eq!(
            texts,
            vec![
                "sys",
                "Summary of the earlier conversation:\nfolded",
                "second old question",
                "second old answer",
                "new question",
                "new tool result",
                "tail question",
            ],
            "the aged entry stays verbatim between summary and window"
        );
        assert_eq!(
            second.messages_after,
            first.messages_after + 1,
            "only the aged entry was added"
        );
    }

    #[tokio::test]
    async fn default_compactor_re_summarizes_once_the_aged_tail_reaches_the_budget() {
        let (model, transcripts) = summarizing_model();
        let settings = ModelSettings::default();
        let summaries = Mutex::new(None);
        let compactor = compactor(&model, &settings, &summaries);

        compactor
            .execute(&run_ctx(), long_history())
            .await
            .expect("first compaction should succeed");
        // Four new turns age four entries past the cached prefix:
        // the budget is reached and one incremental request folds
        // them in.
        let mut grown = long_history();
        for index in 0..4 {
            grown.push(entry(
                RunMessageRole::User,
                &format!("tail question {index}"),
            ));
        }
        compactor
            .execute(&run_ctx(), grown)
            .await
            .expect("second compaction should succeed");

        let transcripts = transcripts.lock().expect("transcripts not poisoned");
        assert_eq!(transcripts.len(), 2, "only the aged tail is re-summarized");
        assert!(
            transcripts[1].contains("Summary so far"),
            "the incremental request must build on the cached summary: {}",
            transcripts[1]
        );
        assert!(
            transcripts[1].contains("second old question"),
            "the incremental request must cover the aged entries: {}",
            transcripts[1]
        );
    }

    #[tokio::test]
    async fn default_compactor_cancels_when_nothing_is_summarizable() {
        let (model, transcripts) = summarizing_model();
        let settings = ModelSettings::default();
        let summaries = Mutex::new(None);
        let short = vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "only turn"),
        ];
        let (outcome, history) = compactor(&model, &settings, &summaries)
            .execute(&run_ctx(), short)
            .await
            .expect("dispatch should not fail");
        assert_eq!(outcome, CompactOutcome::Cancelled);
        assert_eq!(history.len(), 2, "history is returned unchanged");
        assert!(
            transcripts
                .lock()
                .expect("transcripts not poisoned")
                .is_empty(),
            "no summarization may run for a too-short history"
        );
    }

    #[test]
    fn estimate_request_tokens_tracks_serialized_size() {
        // Two empty JSON arrays serialize to four bytes, estimating
        // a single token.
        assert!(estimate_request_tokens(&[], 2) <= 1);
        let messages = vec![ModelRequest::with_parts(vec![
            ModelRequestPart::UserPrompt(UserPromptPart::new("x".repeat(400))),
        ])];
        assert!(
            estimate_request_tokens(&messages, 2) > 0,
            "a non-empty request must estimate tokens"
        );
    }
}
