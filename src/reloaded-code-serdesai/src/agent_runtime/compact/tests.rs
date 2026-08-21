//! Context compaction tests: wrapper thresholds, summarize mapping,
//! fail-open, and stream publication.

use super::*;
use crate::agent_runtime::test_stubs::{
    SerdesTestFactory, agent, allow_tools, catalog, credentials, workspace_root,
};
use crate::agent_runtime::{AgentBuildContext, HookedAgent};
use crate::mock::{FunctionModel, Streamed};
use async_trait::async_trait;
use futures::StreamExt;
use reloaded_code_agents::{AgentCatalog, AgentDefaults, AgentMode, AgentRuntimeBuilder};
use reloaded_code_core::hooks::RunEvent;
use reloaded_code_core::{CompactFraction, CredentialResolver, ToolCatalogEntry, ToolCatalogKind};
use serde_json::json;
use serdes_ai::core::{
    FinishReason, ModelRequestPart, ModelResponsePart, SystemPromptPart, ToolCallPart, UserContent,
    UserPromptPart,
};
use serdes_ai_models::{ModelError, ModelProfile};
use std::sync::{Arc, Mutex};

// ========================================================================
// Fixtures
// ========================================================================

/// Marker inside the oversized prompt every overflow test sends.
const BIG_PROMPT_MARKER: &str = "oversized payload";
/// Marker opening the Core summarization system prompt; every
/// summarize request carries it, so served requests classify by it.
const SUMMARY_PROMPT_MARKER: &str = "You compact a conversation history";

/// Model whose non-streaming requests always fail while its streams
/// keep serving `inner`, so summarize calls fail while the run itself
/// continues on the scripted steps.
struct FailingRequestModel {
    /// Wrapped model serving streams and the reported profile.
    inner: BoxedModel,
}

/// Recorder behind the wrapper-level models: every served request is
/// captured, summarize requests answer `summary_reply`.
struct RecordingModel {
    /// Served requests, in order.
    served: Mutex<Vec<RecordedRequest>>,
    /// Text answering every summarize request.
    summary_reply: String,
}

/// One request the overflow model served.
#[derive(Debug, Clone)]
struct ServedRequest {
    /// Whether the request was a compaction summarize request.
    summary: bool,
    /// Output-token cap the request carried.
    max_tokens: Option<u64>,
    /// Text of the user prompts the request carried.
    user_text: String,
}

/// One request the recording model served.
#[derive(Debug, Clone)]
struct RecordedRequest {
    /// Whether the request was a compaction summarize request.
    summary: bool,
    /// Output-token cap the request carried.
    max_tokens: Option<u64>,
    /// System prompt text of the request's first message.
    system_text: String,
    /// User-prompt text of the request.
    user_text: String,
}

impl FailingRequestModel {
    /// Creates a model failing `request` over `inner`.
    fn new_with_inner(inner: BoxedModel) -> Self {
        Self { inner }
    }

    /// Creates a profile-only model failing `request` and streaming
    /// one text response per step.
    fn new(profile: ModelProfile) -> Self {
        Self {
            inner: Arc::new(
                FunctionModel::new(|_, _| ModelResponse::text("unused")).with_profile(profile),
            ),
        }
    }
}

impl RecordingModel {
    /// Recorder answering summarize requests with `summary_reply`.
    fn new(summary_reply: &str) -> Self {
        Self {
            served: Mutex::new(Vec::new()),
            summary_reply: summary_reply.to_owned(),
        }
    }

    /// Records one served request and picks its reply.
    fn answer(&self, messages: &[ModelRequest], settings: &ModelSettings) -> ModelResponse {
        let summary = messages.iter().any(|message| {
            message
                .system_prompts()
                .any(|part| part.content.contains(SUMMARY_PROMPT_MARKER))
        });
        let user_text = messages
            .iter()
            .flat_map(|message| message.user_prompts())
            .map(|part| part.content.as_text().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        self.served
            .lock()
            .expect("served not poisoned")
            .push(RecordedRequest {
                summary,
                max_tokens: settings.max_tokens,
                system_text: messages
                    .first()
                    .map(|message| {
                        message
                            .system_prompts()
                            .map(|part| part.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
                user_text,
            });
        ModelResponse::text(if summary {
            self.summary_reply.clone()
        } else {
            "unused normal reply".to_owned()
        })
    }

    /// Served summarize requests only.
    fn summaries(&self) -> Vec<RecordedRequest> {
        self.served
            .lock()
            .expect("served not poisoned")
            .iter()
            .filter(|request| request.summary)
            .cloned()
            .collect()
    }

    /// Returns `true` while no request was served.
    fn served_is_empty(&self) -> bool {
        self.served.lock().expect("served not poisoned").is_empty()
    }
}

#[async_trait]
impl serdes_ai_models::Model for FailingRequestModel {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn system(&self) -> &str {
        self.inner.system()
    }

    fn profile(&self) -> &ModelProfile {
        self.inner.profile()
    }

    async fn request(
        &self,
        _messages: &[ModelRequest],
        _settings: &ModelSettings,
        _params: &ModelRequestParameters,
    ) -> Result<ModelResponse, ModelError> {
        Err(ModelError::api("summarize endpoint unavailable"))
    }

    async fn request_stream(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<serdes_ai_models::StreamedResponse, ModelError> {
        self.inner.request_stream(messages, settings, params).await
    }
}

// ========================================================================
// Wrapper-level compaction
// ========================================================================

/// The summarize request's `max_tokens` is the policy cap clamped to
/// the model's advertised output limit.
#[tokio::test]
async fn compact_request_caps_summarize_output_tokens_from_policy_and_model_limit() {
    // (advertised max_output, policy, expected request cap)
    let cases = [
        (None, CompactPolicy::default(), 32_000u64),
        (Some(2_048), CompactPolicy::default(), 2_048),
        (
            Some(8_000),
            CompactPolicy {
                summarize_max_output: 4_096,
                ..CompactPolicy::default()
            },
            4_096,
        ),
    ];
    for (max_output, policy, expected) in cases {
        let recorder = Arc::new(RecordingModel::new("folded"));
        let mut profile = ModelProfile::new().with_context_window(1_024);
        if let Some(max_output) = max_output {
            profile = profile.with_max_tokens(max_output);
        }
        let (model, _records) = wrapped(recorder.clone(), profile, policy);
        compacted(&model, &history(6, 620)).await;

        assert_eq!(
            recorder
                .summaries()
                .first()
                .and_then(|request| request.max_tokens),
            Some(expected),
            "model max_output {max_output:?}"
        );
    }
}

/// A failed summarize request aborts the attempt: the history serves
/// unchanged and no event is queued.
#[tokio::test]
async fn compact_request_fails_open_when_the_summarize_request_errors() {
    let failing = FailingRequestModel::new(ModelProfile::new().with_context_window(1_024));
    let (model, records) =
        CompactModel::new(Arc::new(failing) as BoxedModel, CompactPolicy::default());

    assert!(
        compacted(&model, &history(6, 620)).await.is_none(),
        "a failed summarize request must leave the history unchanged"
    );
    assert!(records.take_pending().is_none(), "no event is queued");
}

/// A summarize response without text aborts the attempt the same
/// way: history unchanged, no event.
#[tokio::test]
async fn compact_request_fails_open_when_the_summary_is_empty() {
    let recorder = Arc::new(RecordingModel::new("   "));
    let (model, records) = wrapped(
        recorder.clone(),
        ModelProfile::new().with_context_window(1_024),
        CompactPolicy::default(),
    );

    assert!(
        compacted(&model, &history(6, 620)).await.is_none(),
        "an empty summary must leave the history unchanged"
    );
    assert_eq!(recorder.summaries().len(), 1, "the request still ran");
    assert!(records.take_pending().is_none(), "no event is queued");
}

/// A fraction override moves the threshold for the same history:
/// past the override yet under the default 3/4 fallback.
#[tokio::test]
async fn compact_request_honors_fraction_override() {
    let messages = history(6, 280);
    // Fixture sanity: the estimate lands clearly past the 1/2
    // override (512) and clearly under the default fallback (768).
    let bytes = serde_json::to_string(&messages).expect("history serializes");
    assert!(
        bytes.len() / 4 > 560 && bytes.len() / 4 < 720,
        "fixture estimate {} tokens must bracket the thresholds",
        bytes.len() / 4
    );

    let recorder = Arc::new(RecordingModel::new("folded"));
    let (default, _records) = wrapped(
        recorder.clone(),
        ModelProfile::new().with_context_window(1_024),
        CompactPolicy::default(),
    );
    assert!(
        compacted(&default, &messages).await.is_none(),
        "the default policy must leave this history unchanged"
    );

    let overridden = CompactPolicy {
        trigger_fraction: CompactFraction::new(1, 2),
        ..CompactPolicy::default()
    };
    let (model, _records) = wrapped(
        recorder,
        ModelProfile::new().with_context_window(1_024),
        overridden,
    );
    assert!(
        compacted(&model, &messages).await.is_some(),
        "the fraction override must compact this history"
    );
}

/// Compaction collapses older entries into one summary entry and
/// keeps the recent window verbatim.
#[tokio::test]
async fn compact_request_keeps_a_recent_message_window() {
    let messages = history(6, 620);
    let recorder = Arc::new(RecordingModel::new("folded"));
    let (model, _records) = wrapped(
        recorder,
        ModelProfile::new().with_context_window(1_024),
        CompactPolicy::default(),
    );

    let served = compacted(&model, &messages)
        .await
        .expect("an over-threshold history must compact");

    // One system entry, one summary entry, the kept window verbatim.
    assert_eq!(served.len(), 6, "six entries survive compaction");
    let summary_text = served[1]
        .system_prompts()
        .map(|part| part.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        summary_text.contains("Summary of the earlier conversation:")
            && summary_text.contains("folded"),
        "the second entry must carry the summary: {summary_text}"
    );
    for (kept, original) in served[2..].iter().zip(&messages[3..]) {
        let kept_text = user_texts(kept);
        let original_text = user_texts(original);
        assert_eq!(kept_text, original_text, "kept entries stay verbatim");
    }
}

/// A context window at or below the margin falls back to the 3/4
/// proportional threshold.
#[tokio::test]
async fn compact_request_maps_small_windows_to_three_quarters() {
    // Threshold: 3/4 of 1_024 = 768 tokens.
    let over = history(6, 620);
    let under = history(6, 360);
    let recorder = Arc::new(RecordingModel::new("folded"));
    let (model, records) = wrapped(
        recorder,
        ModelProfile::new().with_context_window(1_024),
        CompactPolicy::default(),
    );

    assert!(
        compacted(&model, &over).await.is_some(),
        "a small window must compact past its 3/4 share"
    );
    assert!(records.take_pending().is_some(), "one event is queued");
    assert!(
        compacted(&model, &under).await.is_none(),
        "a small window under its 3/4 share must serve unchanged"
    );
}

/// The summarize request carries the Core structured-summary
/// directive and the rendered transcript of the summarized entries.
#[tokio::test]
async fn compact_request_prompt_directs_a_structured_summary() {
    let recorder = Arc::new(RecordingModel::new("folded"));
    let (model, _records) = wrapped(
        recorder.clone(),
        ModelProfile::new().with_context_window(1_024),
        CompactPolicy::default(),
    );

    compacted(&model, &history(6, 620)).await;

    let summaries = recorder.summaries();
    assert_eq!(summaries.len(), 1, "one summarize request");
    let request = &summaries[0];
    assert!(
        request.system_text.contains("<summary>"),
        "the prompt must demand a structured summary block: {}",
        request.system_text
    );
    assert!(
        request.system_text.contains("Task and intent"),
        "the prompt must name the fixed sections: {}",
        request.system_text
    );
    assert!(
        request.system_text.contains("Use the output budget"),
        "detail must be bounded by the output budget: {}",
        request.system_text
    );
    assert!(
        request.user_text.contains("[User]"),
        "the transcript must render the summarized entries: {}",
        request.user_text
    );
}

/// A model with no usable window serves overflowing histories
/// unchanged: no summarize request, no record, and no estimation
/// pass at all.
#[tokio::test]
async fn compact_request_serves_overflowing_history_unchanged_without_a_usable_window() {
    for window in [None, Some(0)] {
        let recorder = Arc::new(RecordingModel::new("folded"));
        let profile = match window {
            Some(limit) => ModelProfile::new().with_context_window(limit),
            None => ModelProfile::new(),
        };
        let (model, records) = wrapped(recorder.clone(), profile, CompactPolicy::default());

        // Three growing step boundaries, each past any threshold a
        // usable window could set.
        for chars in [4_000, 8_000, 16_000] {
            assert!(
                compacted(&model, &history(6, chars)).await.is_none(),
                "window {window:?}: the history must serve unchanged"
            );
        }
        assert!(
            recorder.served_is_empty(),
            "window {window:?}: no summarize request may run"
        );
        assert!(
            records.take_pending().is_none(),
            "window {window:?}: no event is queued"
        );
        assert_eq!(
            model.estimation_passes.load(Ordering::Relaxed),
            0,
            "window {window:?}: no estimation pass may run while no threshold can trigger"
        );
    }
}

/// A short history that crosses the threshold still serves
/// unchanged: nothing is summarizable, so no request runs.
#[tokio::test]
async fn compact_request_serves_short_overflowing_history_unchanged() {
    let messages = history(1, 4_000);
    let recorder = Arc::new(RecordingModel::new("folded"));
    let (model, records) = wrapped(
        recorder.clone(),
        ModelProfile::new().with_context_window(64),
        CompactPolicy::default(),
    );

    assert!(
        compacted(&model, &messages).await.is_none(),
        "a too-short history must serve unchanged"
    );
    assert!(recorder.served_is_empty(), "no summarize request may run");
    assert!(records.take_pending().is_none(), "no event is queued");
}

/// A default-policy request crossing `limit - 32_000` compacts; the
/// same history under the threshold serves unchanged, even where a
/// 3/4 share of the window would already compact.
#[tokio::test]
async fn compact_request_triggers_at_limit_minus_margin_by_default() {
    // Threshold: 160_000 - 32_000 = 128_000 tokens; a 3/4 share is
    // 120_000, so the window separates margin triggering from
    // fraction triggering.
    let over = history(6, 88_000);
    let under = history(6, 82_000);
    // Fixture sanity: the over history clears the margin threshold
    // and the under history sits between the two rules.
    let over_bytes = serde_json::to_string(&over).expect("history serializes");
    assert!(
        over_bytes.len() / 4 > 129_000,
        "over estimate {} must clear the margin threshold",
        over_bytes.len() / 4
    );
    let under_bytes = serde_json::to_string(&under).expect("history serializes");
    assert!(
        under_bytes.len() / 4 > 121_000 && under_bytes.len() / 4 < 127_000,
        "under estimate {} must sit between the margin and 3/4 thresholds",
        under_bytes.len() / 4
    );
    let recorder = Arc::new(RecordingModel::new("folded"));
    let (model, records) = wrapped(
        recorder,
        ModelProfile::new().with_context_window(160_000),
        CompactPolicy::default(),
    );

    assert!(
        compacted(&model, &over).await.is_some(),
        "an over-threshold request must compact"
    );
    assert!(records.take_pending().is_some(), "one event is queued");
    assert!(
        compacted(&model, &under).await.is_none(),
        "a request under the margin threshold must serve unchanged, past a 3/4 share"
    );
    assert!(records.take_pending().is_none(), "no event is queued");
}

// ========================================================================
// Publication gate
// ========================================================================

/// A new stream drops records queued before it started, so it
/// publishes only its own outcomes.
#[test]
fn compaction_gate_drops_records_from_earlier_streams() {
    let records = CompactionRecords::default();
    records.push(record(512));
    let _gate = CompactionGate::new(&records);
    assert!(
        records.take_pending().is_none(),
        "a new stream must not publish outcomes queued before it started"
    );
}

/// `front_matches` anchors on the queue's oldest record estimate.
#[test]
fn compaction_records_front_matches_oldest_record_estimate() {
    let records = CompactionRecords::default();
    assert!(
        !records.front_matches(512),
        "an empty queue anchors nothing"
    );
    records.push(record(512));
    records.push(record(600));
    assert!(records.front_matches(512), "the oldest record anchors");
    assert!(
        !records.front_matches(600),
        "a newer record does not anchor"
    );
}

// ========================================================================
// End-to-end runs
// ========================================================================

/// The non-streaming path keeps compaction internal: the compacted
/// history reaches the model, and no event surface exists. The
/// caller's policy overrides ride the builder into the summarize
/// request.
#[tokio::test]
async fn run_compacts_overflowing_history_internally_without_event_surface() {
    let (model, served) = overflow_scripted_model(1_024, 2);
    let hooked = ping_agent(model)
        .with_compaction(CompactPolicy {
            summarize_max_output: 2_048,
            ..CompactPolicy::default()
        })
        .build("caller")
        .expect("build should succeed");

    let result = hooked
        .run(big_prompt(), ())
        .await
        .expect("run should complete");

    let (normal, summaries) = split_served(&served);
    assert_eq!(summaries, 1, "one summarization request");
    assert_eq!(normal.len(), 3, "three normal requests");
    // The builder forwards the caller's policy, not a default; the
    // scripted profile advertises no output limit, so nothing clamps.
    let summary = served
        .lock()
        .expect("served requests not poisoned")
        .iter()
        .find(|request| request.summary)
        .cloned()
        .expect("one summarization request");
    assert_eq!(summary.max_tokens, Some(2_048));
    assert!(
        !normal[2].user_text.contains(BIG_PROMPT_MARKER),
        "the run path must serve the compacted history: {}",
        normal[2].user_text
    );
    assert!(
        !result.output().is_empty(),
        "the run should still produce output"
    );
}

/// Repeated over-threshold steps amortize summarization through the
/// summary cache: one request per tail budget, not one per step.
#[tokio::test]
async fn run_stream_amortizes_summarization_over_repeated_overflow_steps() {
    let (model, served) = overflow_scripted_model(1_024, 6);
    let hooked = ping_agent(Streamed::new(model))
        .with_compaction(CompactPolicy::default())
        .build("caller")
        .expect("build should succeed");

    let events = collect_events(&hooked, big_prompt()).await;
    assert!(
        matches!(events.last(), Some(RunEvent::RunComplete { .. })),
        "the stream should complete: {events:?}"
    );

    let (normal, summaries) = split_served(&served);
    assert_eq!(normal.len(), 7, "one request per scripted step");
    assert_eq!(
        summaries, 3,
        "summarization must amortize to one request per tail budget, \
         not one per over-threshold step (5)"
    );
    let compressed = events
        .iter()
        .filter(|event| matches!(event, RunEvent::ContextCompressed { .. }))
        .count();
    assert_eq!(
        compressed, 5,
        "every over-threshold step from the third on applies one compaction"
    );
}

/// A streamed run past the threshold publishes exactly one
/// `ContextCompressed` between the compacted step's `ContextInfo` and
/// that step's content, and the model serves the compacted history.
#[tokio::test]
async fn run_stream_emits_context_compressed_between_context_info_and_content() {
    let (model, served) = overflow_scripted_model(1_024, 2);
    let hooked = ping_agent(Streamed::new(model))
        .with_compaction(CompactPolicy::default())
        .build("caller")
        .expect("build should succeed");

    let events = collect_events(&hooked, big_prompt()).await;
    assert!(
        matches!(events.last(), Some(RunEvent::RunComplete { .. })),
        "the stream should complete: {events:?}"
    );

    // Exactly one compaction: the overflowing third request.
    let compressed: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            matches!(event, RunEvent::ContextCompressed { .. }).then_some(index)
        })
        .collect();
    assert_eq!(compressed.len(), 1, "one compaction event: {events:?}");
    let compressed = compressed[0];

    // The event's fields mirror the applied compaction.
    let RunEvent::ContextCompressed {
        original_tokens,
        compressed_tokens,
        strategy,
        messages_before,
        messages_after,
    } = &events[compressed]
    else {
        unreachable!("filtered to ContextCompressed");
    };
    assert_eq!(strategy, "summarize");
    assert_eq!(*messages_before, 6);
    assert_eq!(*messages_after, 6);
    assert!(
        *original_tokens > *compressed_tokens && *compressed_tokens > 0,
        "compaction must shrink the estimate: {original_tokens} -> {compressed_tokens}"
    );

    // Deterministic ordering: after the third step's ContextInfo,
    // before that step's first text delta.
    let info_positions: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| matches!(event, RunEvent::ContextInfo { .. }).then_some(index))
        .collect();
    assert_eq!(info_positions.len(), 3, "one context info per step");
    let third_info = info_positions[2];
    let first_delta_after = events[third_info + 1..]
        .iter()
        .position(|event| matches!(event, RunEvent::TextDelta { .. }))
        .expect("the final step should stream text")
        + third_info
        + 1;
    assert!(
        third_info < compressed && compressed < first_delta_after,
        "ContextCompressed must land between ContextInfo and the step content"
    );

    // The compacted request the model serves carries the summary
    // instead of the oversized prompt, and the summarize request
    // carried the default policy cap.
    let (normal, summaries) = split_served(&served);
    assert_eq!(summaries, 1, "one summarization request");
    assert_eq!(normal.len(), 3, "three normal requests");
    let summary = served
        .lock()
        .expect("served requests not poisoned")
        .iter()
        .find(|request| request.summary)
        .cloned()
        .expect("one summarization request");
    assert_eq!(summary.max_tokens, Some(32_000));
    assert!(
        !normal[2].user_text.contains(BIG_PROMPT_MARKER),
        "the oversized prompt must not reach the model uncompacted: {}",
        normal[2].user_text
    );
}

/// A failed summarize request fails open: the run continues on the
/// original history and no compaction event publishes.
#[tokio::test]
async fn run_stream_fails_open_when_the_summarize_request_errors() {
    let (scripted, served) = overflow_scripted_model(1_024, 2);
    let failing = FailingRequestModel::new_with_inner(Arc::new(Streamed::new(scripted)));
    let hooked = ping_agent(failing)
        .with_compaction(CompactPolicy::default())
        .build("caller")
        .expect("build should succeed");

    let events = collect_events(&hooked, big_prompt()).await;
    assert!(
        matches!(events.last(), Some(RunEvent::RunComplete { .. })),
        "the run must continue after a failed attempt: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, RunEvent::ContextCompressed { .. })),
        "a failed attempt must publish no event"
    );

    // The overflowing request was served unchanged: the history still
    // carries the oversized prompt, verbatim.
    let (normal, summaries) = split_served(&served);
    assert_eq!(summaries, 0, "no summarization may succeed");
    assert_eq!(normal.len(), 3, "three normal requests");
    assert!(
        normal[2].user_text.contains(BIG_PROMPT_MARKER),
        "a failed attempt must leave the history unchanged: {}",
        normal[2].user_text
    );
}

/// A build that leaves compaction disabled performs no compaction:
/// the overflowing request serves unchanged and no event appears.
#[tokio::test]
async fn run_stream_without_compaction_serves_overflowing_history_unchanged() {
    let (model, served) = overflow_scripted_model(1_024, 2);
    let hooked = ping_agent(Streamed::new(model))
        .build("caller")
        .expect("build should succeed");

    let events = collect_events(&hooked, big_prompt()).await;
    assert!(
        matches!(events.last(), Some(RunEvent::RunComplete { .. })),
        "the stream should complete: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, RunEvent::ContextCompressed { .. })),
        "no compaction event may appear on a disabled build"
    );

    // Without the wrapper there is no estimation and no summarize
    // request: the overflowing history reaches the model verbatim.
    let (normal, summaries) = split_served(&served);
    assert_eq!(summaries, 0, "no summarization may run");
    assert_eq!(normal.len(), 3, "three normal requests");
    assert!(
        normal[2].user_text.contains(BIG_PROMPT_MARKER),
        "the overflowing request must be served unchanged: {}",
        normal[2].user_text
    );
}

// ========================================================================
// Helpers
// ========================================================================

/// Builds the oversized prompt every overflow test sends.
fn big_prompt() -> String {
    format!("{BIG_PROMPT_MARKER}. {}", "filler sentence. ".repeat(400))
}

/// Collects every event of one `run_stream` call into owned events.
async fn collect_events(agent: &HookedAgent, prompt: impl Into<UserContent>) -> Vec<RunEvent> {
    let mut stream = agent
        .run_stream(prompt, ())
        .await
        .expect("stream should start");
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.expect("stream item should be ok"));
    }
    events
}

/// Compact result of one wrapper request, or `None` when the request
/// must be served unchanged.
async fn compacted(model: &CompactModel, messages: &[ModelRequest]) -> Option<Vec<ModelRequest>> {
    model
        .compact_request(
            messages,
            &ModelSettings::default(),
            &ModelRequestParameters::default(),
        )
        .await
}

/// Builds a history of one system entry plus `turns` user turns of
/// `chars` text characters each.
fn history(turns: usize, chars: usize) -> Vec<ModelRequest> {
    let mut messages = vec![ModelRequest::with_parts(vec![
        ModelRequestPart::SystemPrompt(SystemPromptPart::new("sys")),
    ])];
    for _ in 0..turns {
        messages.push(ModelRequest::with_parts(vec![
            ModelRequestPart::UserPrompt(UserPromptPart::new(UserContent::Text("x".repeat(chars)))),
        ]));
    }
    messages
}

/// Builds a scripted model with a `context_window` of `limit` tokens,
/// recording every request it serves. The first `ping_rounds` normal
/// requests call `ping`; later ones answer with text. Requests
/// carrying the summarization system prompt answer "folded history".
fn overflow_scripted_model(
    limit: u64,
    ping_rounds: usize,
) -> (FunctionModel, Arc<Mutex<Vec<ServedRequest>>>) {
    let served: Arc<Mutex<Vec<ServedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&served);
    let model = FunctionModel::new(move |messages, settings| {
        let summary = messages.iter().any(|message| {
            message
                .system_prompts()
                .any(|part| part.content.contains(SUMMARY_PROMPT_MARKER))
        });
        let user_text = messages
            .iter()
            .flat_map(|message| message.user_prompts())
            .map(|part| part.content.as_text().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
            .join("\n");
        seen.lock()
            .expect("served requests should not be poisoned")
            .push(ServedRequest {
                summary,
                max_tokens: settings.max_tokens,
                user_text,
            });
        if summary {
            return ModelResponse::text("folded history");
        }
        let normal_seen = seen
            .lock()
            .expect("served requests should not be poisoned")
            .iter()
            .filter(|request| !request.summary)
            .count();
        if normal_seen <= ping_rounds {
            ModelResponse::with_parts(vec![ModelResponsePart::ToolCall(
                ToolCallPart::new("ping", json!({"target": "example.com"}))
                    .with_tool_call_id(format!("call_mock_{normal_seen}")),
            )])
            .with_finish_reason(FinishReason::ToolCall)
        } else {
            ModelResponse::text("after the tools")
        }
    })
    .with_profile(ModelProfile::new().with_context_window(limit));
    (model, served)
}

/// Builds an uncompiled context for a `caller` agent whose only tool
/// is a custom `ping` tool returning "pong", running `model`.
fn ping_agent(
    model: impl serdes_ai_models::Model + 'static,
) -> AgentBuildContext<CredentialResolver<false>> {
    let runtime = AgentRuntimeBuilder::new()
        .catalog(AgentCatalog::from_entries([agent(
            "caller",
            AgentMode::Primary,
            allow_tools(&["ping"]),
            "prompt",
        )]))
        .tools(vec![ToolCatalogEntry::new("ping", ToolCatalogKind::Custom)])
        .custom_tool(SerdesTestFactory::new(
            "ping",
            "Use ping to check connectivity.",
            "pong",
        ))
        .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
        .build()
        .expect("runtime should build");

    AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(catalog()),
        Arc::new(credentials()),
        workspace_root(),
    )
    .with_model_override(model)
}

/// A record applied to a core compaction.
fn record(tokens_before: usize) -> CompactionRecord {
    CompactionRecord {
        summary: "folded".into(),
        tokens_before,
        tokens_after: tokens_before / 8,
        strategy: "summarize".into(),
        messages_before: 6,
        messages_after: 6,
    }
}

/// Splits served requests into normal requests and summary count.
fn split_served(served: &Arc<Mutex<Vec<ServedRequest>>>) -> (Vec<ServedRequest>, usize) {
    let served = served.lock().expect("served requests not poisoned");
    let normal: Vec<ServedRequest> = served
        .iter()
        .filter(|request| !request.summary)
        .cloned()
        .collect();
    let summaries = served.iter().filter(|request| request.summary).count();
    (normal, summaries)
}

/// Joined user-prompt text of one request.
fn user_texts(request: &ModelRequest) -> String {
    request
        .user_prompts()
        .map(|part| part.content.as_text().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wraps a recording model with `profile` for compaction under
/// `policy`, returning the wrapper and its records.
fn wrapped(
    recorder: Arc<RecordingModel>,
    profile: ModelProfile,
    policy: CompactPolicy,
) -> (CompactModel, CompactionRecords) {
    let model = FunctionModel::new(move |messages, settings| recorder.answer(messages, settings))
        .with_profile(profile);
    CompactModel::new(Arc::new(model), policy)
}
