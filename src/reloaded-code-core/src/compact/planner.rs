//! Compaction planning: kept-window split, summary memoization, and
//! the applied compaction's record.

use super::entry::CompactEntry;
use super::policy::CompactPolicy;
use super::port::{SummaryExecutor, SummaryRequest};
use crate::hooks::RunMessageRole;
use crate::ToolError;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem;
use std::sync::Mutex;

/// Most-recent non-system entries the planner keeps verbatim.
const KEEP_RECENT_ENTRIES: usize = 4;
/// New summarizable entries that accumulate before the cached
/// summary is refreshed with one request. Below it, the new entries
/// stay verbatim after the summary, so a grown prefix costs no
/// request.
const RE_SUMMARY_TAIL_ENTRIES: usize = 4;
/// Strategy name the summarize planner reports.
const STRATEGY_SUMMARIZE: &str = "summarize";
/// Label introducing the summary inside the compacted history.
const SUMMARY_PREFIX: &str = "Summary of the earlier conversation:";
/// System prompt for summarize requests. Detail is bounded only by
/// the request's output-token cap, so the model keeps as much
/// information as the budget allows instead of a terse digest.
const SUMMARY_SYSTEM_PROMPT: &str = "You compact conversation history. Produce the longest, \
     most detailed summary the output budget allows: retain as much information as possible, \
     including decisions, facts, open questions, and tool results. Do not shorten the summary \
     to be brief; use the available output budget when the history justifies it. Reply with \
     the summary text only.";

/// One applied compaction: the compacted history plus its record.
#[derive(Debug)]
pub struct Compaction {
    /// Compacted history replacing the compacted input.
    pub history: Vec<CompactEntry>,
    /// Record describing the applied compaction.
    pub record: CompactionRecord,
}

/// Plans and applies compactions over neutral entries.
///
/// Owns the policy, the model's advertised output limit, and the
/// summary cache. Each [`Self::compact`] call splits the history,
/// reuses the cached summary when it can, and asks the
/// [`SummaryExecutor`] port for the summarize requests it still
/// needs, so the planner itself stays free of model knowledge.
pub struct Compactor {
    policy: CompactPolicy,
    /// Model's advertised maximum output tokens, when known; the
    /// summarize cap clamps to it.
    max_output: Option<u64>,
    /// Memoized summary, keyed on the summarized prefix.
    summaries: Mutex<Option<CachedSummary>>,
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
struct CachedSummary {
    /// Fingerprints of the summarized entries.
    fingerprints: Vec<u64>,
    /// Summary text covering those entries.
    summary: String,
}

/// Record of one applied compaction.
///
/// The counts and names are advisory: they describe the attempt so
/// callers can report it. `tokens_before` and `tokens_after` become
/// the token counts of [`RunEvent::ContextCompressed`];
/// `strategy`, `messages_before`, and `messages_after` map field for
/// field. `summary` has no counterpart on the event.
///
/// [`RunEvent::ContextCompressed`]: crate::hooks::RunEvent::ContextCompressed
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionRecord {
    /// Summary text replacing the compacted messages.
    pub summary: String,
    /// Token count of the history before compaction.
    pub tokens_before: usize,
    /// Estimated token count of the history after compaction.
    pub tokens_after: usize,
    /// Strategy name, e.g. "summarize".
    pub strategy: String,
    /// Number of messages before compaction.
    pub messages_before: usize,
    /// Number of messages after compaction.
    pub messages_after: usize,
}

impl Compactor {
    /// Assembles a compactor over `policy` and the model's
    /// advertised `max_output`.
    ///
    /// `max_output`, when known, clamps the policy's summarize cap.
    #[must_use]
    pub fn new(policy: CompactPolicy, max_output: Option<u64>) -> Self {
        Self {
            policy,
            max_output,
            summaries: Mutex::new(None),
        }
    }

    /// Compacts `history`, returning the compacted history with its
    /// record, or `None` while nothing is summarizable.
    ///
    /// Leading system entries stay, older entries collapse into one
    /// summary entry, and a recent window stays verbatim. Repeated
    /// calls memoize on the summarized prefix: an unchanged prefix
    /// costs no request, a small grown tail stays verbatim without
    /// one, and a tail past the re-summary budget folds into the
    /// summary with one request.
    ///
    /// `tokens_before` is the caller's request-level token estimate
    /// of the uncompacted history.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the summarize request fails or
    /// returns no text; the caller serves the original history
    /// unchanged.
    pub async fn compact(
        &self,
        executor: &dyn SummaryExecutor,
        history: Vec<CompactEntry>,
        tokens_before: usize,
    ) -> Result<Option<Compaction>, ToolError> {
        let mut history = history;
        let messages_before = history.len();
        let leading_systems = leading_system_count(&history);
        let Some(kept_start) = kept_window_start(&history) else {
            // Nothing summarizable: apply no history change.
            return Ok(None);
        };
        let kept = history.split_off(kept_start);
        // The summarize side moves out by value: entry slices are
        // not shareable across await points.
        let older = history.split_off(leading_systems);
        let (summary, tail) = self.summarize(executor, older).await?;
        history.push(CompactEntry::new(
            RunMessageRole::System,
            format!("{SUMMARY_PREFIX}\n{summary}"),
        ));
        history.extend(tail);
        history.extend(kept);
        let tokens_after = estimate_entry_tokens(&history);
        let record = CompactionRecord {
            summary,
            tokens_before,
            tokens_after,
            strategy: STRATEGY_SUMMARIZE.to_owned(),
            messages_before,
            messages_after: history.len(),
        };
        Ok(Some(Compaction { history, record }))
    }

    /// Summarizes `entries`, memoized on the entry fingerprints.
    ///
    /// Returns the summary text plus the entries it does not cover:
    /// a grown prefix under the tail budget keeps its new entries
    /// verbatim instead of paying a model request.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the summarize request fails or
    /// returns no text.
    async fn summarize(
        &self,
        executor: &dyn SummaryExecutor,
        entries: Vec<CompactEntry>,
    ) -> Result<(String, Vec<CompactEntry>), ToolError> {
        let fingerprints: Vec<u64> = entries.iter().map(entry_fingerprint).collect();
        // The lock only picks the plan; the request runs unlocked.
        let plan = {
            let cached = self
                .summaries
                .lock()
                .expect("summary cache should not be poisoned");
            select_plan(cached.as_ref(), &fingerprints)
        };
        let mut entries = entries;
        match plan {
            SummarizePlan::Reuse { summary } => Ok((summary, Vec::new())),
            SummarizePlan::Tail { summary, since } => {
                let tail = entries.split_off(since);
                // The cache still describes the summarized prefix;
                // leave it for the next, larger attempt.
                Ok((summary, tail))
            }
            SummarizePlan::Incremental { previous, since } => {
                let delta = format!(
                    "Summary so far:\n{previous}\n\nMessages since:\n{}",
                    render_transcript(&entries[since..]),
                );
                let summary = self.request_summary(executor, delta).await?;
                self.store(fingerprints, &summary);
                Ok((summary, Vec::new()))
            }
            SummarizePlan::Full => {
                let summary = self
                    .request_summary(executor, render_transcript(&entries))
                    .await?;
                self.store(fingerprints, &summary);
                Ok((summary, Vec::new()))
            }
        }
    }

    /// Requests one summary through the port.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the request fails or the response
    /// carries no text.
    async fn request_summary(
        &self,
        executor: &dyn SummaryExecutor,
        transcript: String,
    ) -> Result<String, ToolError> {
        let request = SummaryRequest {
            system_prompt: SUMMARY_SYSTEM_PROMPT,
            transcript,
            max_output_tokens: self.policy.summarize_cap(self.max_output),
        };
        let summary = executor.summarize(request).await.map_err(|error| {
            ToolError::Execution(format!(
                "context compaction summary request failed: {error}"
            ))
        })?;
        if summary.trim().is_empty() {
            return Err(ToolError::Execution(
                "context compaction summary was empty".into(),
            ));
        }
        Ok(summary)
    }

    /// Memoizes `summary` over `fingerprints`.
    fn store(&self, fingerprints: Vec<u64>, summary: &str) {
        *self
            .summaries
            .lock()
            .expect("summary cache should not be poisoned") = Some(CachedSummary {
            fingerprints,
            summary: summary.to_owned(),
        });
    }
}

/// Stable fingerprint of one entry's summary-relevant content.
fn entry_fingerprint(entry: &CompactEntry) -> u64 {
    let mut hasher = DefaultHasher::new();
    mem::discriminant(&entry.role()).hash(&mut hasher);
    entry.text().hash(&mut hasher);
    entry.has_preserved().hash(&mut hasher);
    hasher.finish()
}

/// Estimated tokens of an entry view: text bytes over four.
fn estimate_entry_tokens(entries: &[CompactEntry]) -> usize {
    entries
        .iter()
        .map(|entry| entry.text().len())
        .sum::<usize>()
        / 4
}

/// Index where the kept window starts, or `None` while the history
/// is too short to compact.
///
/// Leading system entries are never summarized; the window covers
/// the last [`KEEP_RECENT_ENTRIES`] entries after them. A tool entry
/// at the window start answers a call that would be summarized, so
/// the start moves past such entries.
fn kept_window_start(entries: &[CompactEntry]) -> Option<usize> {
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

/// Renders entries as the transcript given to the summarizer.
fn render_transcript(entries: &[CompactEntry]) -> String {
    // Longest role tag: brackets, name, space, and newline.
    const ROLE_TAG_LEN: usize = "[Assistant] \n".len();
    let text_len: usize = entries.iter().map(|entry| entry.text().len()).sum();
    let mut rendered = String::with_capacity(text_len + entries.len() * ROLE_TAG_LEN);
    for entry in entries {
        let role = match entry.role() {
            RunMessageRole::System => "System",
            RunMessageRole::User => "User",
            RunMessageRole::Assistant => "Assistant",
            RunMessageRole::Tool => "Tool",
        };
        rendered.push('[');
        rendered.push_str(role);
        rendered.push_str("] ");
        rendered.push_str(entry.text());
        rendered.push('\n');
    }
    rendered
}

/// Picks the reuse plan for `fingerprints` against the cached
/// summary, cloning the cached text the chosen plan needs.
fn select_plan(cached: Option<&CachedSummary>, fingerprints: &[u64]) -> SummarizePlan {
    match cached {
        Some(cached) if cached.fingerprints.as_slice() == fingerprints => SummarizePlan::Reuse {
            summary: cached.summary.clone(),
        },
        Some(cached)
            if fingerprints.len() > cached.fingerprints.len()
                && fingerprints[..cached.fingerprints.len()] == cached.fingerprints[..] =>
        {
            let since = cached.fingerprints.len();
            if fingerprints.len() - since < RE_SUMMARY_TAIL_ENTRIES {
                SummarizePlan::Tail {
                    summary: cached.summary.clone(),
                    since,
                }
            } else {
                SummarizePlan::Incremental {
                    previous: cached.summary.clone(),
                    since,
                }
            }
        }
        _ => SummarizePlan::Full,
    }
}

/// Number of leading system entries.
fn leading_system_count(entries: &[CompactEntry]) -> usize {
    entries
        .iter()
        .take_while(|entry| entry.role() == RunMessageRole::System)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::SummaryFuture;
    use std::sync::Arc;

    /// History long enough to compact: one system entry, two older
    /// turns to summarize, then a four-entry kept window.
    fn long_history() -> Vec<CompactEntry> {
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

    fn entry(role: RunMessageRole, text: &str) -> CompactEntry {
        CompactEntry::new(role, text)
    }

    /// Port recording each request and answering `reply`.
    struct RecordingExecutor {
        requests: Mutex<Vec<SummaryRequest>>,
        reply: String,
        failure: Option<String>,
    }

    impl RecordingExecutor {
        /// Port answering every request with fixed summary text.
        fn replying(reply: &str) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                reply: reply.to_owned(),
                failure: None,
            }
        }

        /// Port failing every request.
        fn failing(message: &str) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                reply: String::new(),
                failure: Some(message.to_owned()),
            }
        }

        /// Captured requests, cloned out of the recorder.
        fn requests(&self) -> Vec<SummaryRequest> {
            self.requests
                .lock()
                .expect("recorded requests should not be poisoned")
                .clone()
        }
    }

    impl SummaryExecutor for RecordingExecutor {
        fn summarize<'a>(&'a self, request: SummaryRequest) -> SummaryFuture<'a> {
            self.requests
                .lock()
                .expect("recorder should not be poisoned")
                .push(request);
            let reply = self.reply.clone();
            let failure = self.failure.clone();
            Box::pin(async move {
                match failure {
                    Some(message) => Err(ToolError::Execution(message)),
                    None => Ok(reply),
                }
            })
        }
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
    fn select_plan_summarizes_from_scratch_when_the_prefix_changes() {
        let cached = CachedSummary {
            fingerprints: vec![10, 11],
            summary: "old".into(),
        };
        // A different first entry, and a shrunk prefix, both lose
        // the cached summary.
        assert!(matches!(
            select_plan(Some(&cached), &[99, 11]),
            SummarizePlan::Full
        ));
        assert!(matches!(
            select_plan(Some(&cached), &[10]),
            SummarizePlan::Full
        ));
        // No cache at all summarizes everything.
        assert!(matches!(select_plan(None, &[10]), SummarizePlan::Full));
    }

    #[test]
    fn select_plan_pins_the_re_summary_tail_budget_boundary() {
        // Three aged entries stay verbatim under the budget of four;
        // the fourth folds them into a re-summary. Pinning both
        // sides keeps the budget constant from shrinking or growing
        // unnoticed.
        let cached = CachedSummary {
            fingerprints: vec![10, 11],
            summary: "old".into(),
        };
        let aged_three = [10, 11, 100, 101, 102];
        let aged_four = [10, 11, 100, 101, 102, 103];
        assert!(matches!(
            select_plan(Some(&cached), &aged_three),
            SummarizePlan::Tail { .. }
        ));
        assert!(matches!(
            select_plan(Some(&cached), &aged_four),
            SummarizePlan::Incremental { .. }
        ));
    }

    #[test]
    fn entry_fingerprint_tracks_summary_relevant_content() {
        let base = entry(RunMessageRole::User, "question");
        let same = entry(RunMessageRole::User, "question");
        assert_eq!(entry_fingerprint(&base), entry_fingerprint(&same));
        let other_text = entry(RunMessageRole::User, "other question");
        assert_ne!(entry_fingerprint(&base), entry_fingerprint(&other_text));
        let other_role = entry(RunMessageRole::Assistant, "question");
        assert_ne!(entry_fingerprint(&base), entry_fingerprint(&other_role));
    }

    #[test]
    fn render_transcript_labels_each_entry_role() {
        let rendered = render_transcript(&[
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "hello"),
        ]);
        assert_eq!(rendered, "[System] sys\n[User] hello\n");
    }

    #[tokio::test]
    async fn compact_summarizes_old_entries_and_keeps_the_window() {
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        let attempt = compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("compaction should succeed")
            .expect("the history is long enough");

        assert_eq!(attempt.record.strategy, "summarize");
        assert_eq!(attempt.record.messages_before, 7);
        assert_eq!(
            attempt.record.messages_after, 6,
            "two summarized entries collapse into one summary entry"
        );
        assert_eq!(attempt.record.tokens_before, 100);
        assert_eq!(attempt.record.summary, "folded");
        assert!(
            attempt.record.tokens_after < 100,
            "the compacted view must estimate smaller"
        );
        // History: leading system, summary entry, kept window
        // verbatim.
        let texts: Vec<&str> = attempt.history.iter().map(CompactEntry::text).collect();
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
        assert_eq!(executor.requests().len(), 1, "one summarize request");
    }

    #[tokio::test]
    async fn compact_prompt_directs_a_maximally_detailed_summary() {
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("compaction should succeed");

        let requests = executor.requests();
        assert!(
            requests[0].system_prompt.contains("most detailed summary"),
            "the prompt must demand maximal detail: {}",
            requests[0].system_prompt
        );
        assert!(
            requests[0].system_prompt.contains("output budget allows"),
            "detail must be bounded by the output budget: {}",
            requests[0].system_prompt
        );
        assert!(
            requests[0].transcript.contains("[User] old question"),
            "the transcript must render the summarized entries: {}",
            requests[0].transcript
        );
    }

    #[tokio::test]
    async fn compact_reuses_summary_for_unchanged_prefix() {
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("first compaction should succeed");
        // Second attempt over the same entries: no new request.
        compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("second compaction should succeed");
        assert_eq!(
            executor.requests().len(),
            1,
            "an unchanged prefix must reuse the cached summary"
        );
    }

    #[tokio::test]
    async fn compact_keeps_small_grown_tail_verbatim_without_new_request() {
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        let first = compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("first compaction should succeed")
            .expect("the history is long enough");
        // A new turn slides the kept window forward by one entry:
        // under the tail budget, no model request runs and the aged
        // entry stays verbatim after the cached summary.
        let mut grown = long_history();
        grown.push(entry(RunMessageRole::User, "tail question"));
        let second = compactor
            .compact(&executor, grown, 100)
            .await
            .expect("second compaction should succeed")
            .expect("the grown history is long enough");

        assert_eq!(
            executor.requests().len(),
            1,
            "a small grown tail must not pay a summarize request"
        );
        let texts: Vec<&str> = second.history.iter().map(CompactEntry::text).collect();
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
            second.record.messages_after,
            first.record.messages_after + 1,
            "only the aged entry was added"
        );
    }

    #[tokio::test]
    async fn compact_re_summarizes_once_the_aged_tail_reaches_the_budget() {
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        compactor
            .compact(&executor, long_history(), 100)
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
            .compact(&executor, grown, 100)
            .await
            .expect("second compaction should succeed");

        let requests = executor.requests();
        assert_eq!(requests.len(), 2, "only the aged tail is re-summarized");
        assert!(
            requests[1].transcript.contains("Summary so far"),
            "the incremental request must build on the cached summary: {}",
            requests[1].transcript
        );
        assert!(
            requests[1]
                .transcript
                .contains("[User] second old question"),
            "the incremental request must cover the aged entries: {}",
            requests[1].transcript
        );
    }

    #[tokio::test]
    async fn compact_returns_none_when_nothing_is_summarizable() {
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);
        let short = vec![
            entry(RunMessageRole::System, "sys"),
            entry(RunMessageRole::User, "only turn"),
        ];

        let attempt = compactor
            .compact(&executor, short, 100)
            .await
            .expect("a too-short history is not an error");
        assert!(attempt.is_none(), "no history change may apply");
        assert!(
            executor.requests().is_empty(),
            "no summarize request may run for a too-short history"
        );
    }

    #[tokio::test]
    async fn compact_caps_summarize_output_tokens_from_policy_and_model_limit() {
        // A clamped model limit wins over the default cap.
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), Some(8_000));
        compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("compaction should succeed");
        assert_eq!(executor.requests()[0].max_output_tokens, 8_000);

        // A policy override applies while the model limit is absent.
        let policy = CompactPolicy {
            summarize_max_output: 16_000,
            ..CompactPolicy::default()
        };
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(policy, None);
        compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect("compaction should succeed");
        assert_eq!(executor.requests()[0].max_output_tokens, 16_000);
    }

    #[tokio::test]
    async fn compact_fails_when_the_summarize_request_fails() {
        let executor = RecordingExecutor::failing("model down");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        let error = compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect_err("a failed request must fail the attempt");
        assert!(
            error.to_string().contains("summary request failed"),
            "the error must name the failed request: {error}"
        );
    }

    #[tokio::test]
    async fn compact_fails_when_the_summary_is_empty() {
        let executor = RecordingExecutor::replying("   ");
        let compactor = Compactor::new(CompactPolicy::default(), None);

        let error = compactor
            .compact(&executor, long_history(), 100)
            .await
            .expect_err("an empty summary must fail the attempt");
        assert!(
            error.to_string().contains("summary was empty"),
            "the error must name the empty summary: {error}"
        );
    }

    #[tokio::test]
    async fn compact_serves_leading_systems_verbatim() {
        // Leading system entries are never summarized: they all stay
        // ahead of the summary entry.
        let executor = RecordingExecutor::replying("folded");
        let compactor = Compactor::new(CompactPolicy::default(), None);
        let mut history = vec![entry(RunMessageRole::System, "sys one")];
        history.extend(long_history());
        history.insert(1, entry(RunMessageRole::System, "sys two"));

        let attempt = compactor
            .compact(&executor, history, 100)
            .await
            .expect("compaction should succeed")
            .expect("the history is long enough");

        let texts: Vec<&str> = attempt.history.iter().map(CompactEntry::text).collect();
        assert_eq!(
            texts,
            vec![
                "sys one",
                "sys two",
                "sys",
                "Summary of the earlier conversation:\nfolded",
                "second old question",
                "second old answer",
                "new question",
                "new tool result",
            ]
        );
        assert!(
            !executor.requests()[0].transcript.contains("[System] sys"),
            "leading system entries must not reach the summarizer: {}",
            executor.requests()[0].transcript
        );
    }

    #[tokio::test]
    async fn compactor_serves_concurrent_attempts_over_one_cache() {
        // The cache lock only picks the plan, so two in-flight
        // attempts over one compactor never deadlock; both see an
        // empty cache or a stored one, paying at most two requests.
        let executor = RecordingExecutor::replying("folded");
        let compactor = Arc::new(Compactor::new(CompactPolicy::default(), None));
        let first = compactor.clone();
        let second = compactor.clone();
        let (a, b) = tokio::join!(
            first.compact(&executor, long_history(), 100),
            second.compact(&executor, long_history(), 100)
        );
        a.expect("the first concurrent compaction should succeed")
            .expect("the history is long enough");
        b.expect("the second concurrent compaction should succeed")
            .expect("the history is long enough");
        assert!(
            (1..=2).contains(&executor.requests().len()),
            "both attempts share one cache: {} requests",
            executor.requests().len()
        );
    }
}
