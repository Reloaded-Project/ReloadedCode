//! Step-boundary context compaction for the SerdesAI run loop.
//!
//! # Wiring
//!
//! Both vendor run paths call the model once per step with the whole
//! conversation, so the model boundary is the public seam where the
//! wrapper sees each step's history. When compact hooks are
//! registered, `build_agent` wraps the resolved model in
//! [`CompactModel`]; with no compact hooks the agent keeps its model
//! untouched and nothing here runs.
//!
//! Each model request checks context usage with the same heuristic
//! the vendor's `ContextInfo` uses: serialized request bytes over
//! four, against the model profile's context window. Past the
//! threshold, the request projects to [`CompactMessage`] entries and
//! dispatches the compact chain; the default compaction in
//! [`history`] summarizes the older entries through the wrapped
//! model and keeps a recent window verbatim. The compacted history
//! becomes the request the model serves, which applies the outcome
//! without touching vendor internals.
//!
//! # Events and failure
//!
//! Each applied compaction records its [`CompactResult`]; the
//! `run_stream` wrapper publishes pending records as
//! [`RunEvent::ContextCompressed`] once their step's `ContextInfo`
//! has published. The `run()` path keeps compaction internal by
//! design: history effect only, no event surface.
//!
//! A cancelled outcome, a hook error, or a failed summarization
//! request aborts the attempt: the original history is served
//! unchanged and no event is recorded, so the run continues.
//!
//! # Remarks
//!
//! One record queue and one summary cache are shared by every run of
//! the agent, so concurrent runs through one agent get best-effort
//! attribution.
//!
//! Next: see `build_agent` in [`super::task`] for the wrap point.
//!
//! [`CompactMessage`]: reloaded_code_core::hooks::CompactMessage
//! [`CompactResult`]: reloaded_code_core::hooks::CompactResult
//! [`RunEvent::ContextCompressed`]: reloaded_code_core::hooks::RunEvent::ContextCompressed

use history::{CachedSummary, DefaultCompactor};
use reloaded_code_core::hooks::{CompactOutcome, CompactResult, HookRunContext, HookSet};
use serdes_ai::AgentStreamEvent;
use serdes_ai::core::{ModelRequest, ModelResponse, ModelSettings};
use serdes_ai_models::{BoxedModel, Model, ModelProfile, ModelRequestParameters};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

mod history;

/// Denominator of the overflow fraction; see [`OVERFLOW_NUMERATOR`].
const OVERFLOW_DENOMINATOR: u64 = 4;
/// Compaction triggers once estimated request tokens reach this
/// fraction of the model's context window.
const OVERFLOW_NUMERATOR: u64 = 3;
/// Bound on undelivered compaction records; the oldest drop first.
/// One record per overflowing step keeps realistic streams far below
/// this, and `run()` runs, which drain nothing, stay bounded.
const PENDING_RECORDS_CAP: usize = 1024;
/// System prompt marking the default compaction's summarization
/// request.
pub(super) const SUMMARY_SYSTEM_PROMPT: &str = "You compact conversation history. Summarize \
     the earlier messages, keeping decisions, facts, open questions, and tool results. Reply \
     with the summary text only.";

/// Model wrapper running context compaction at each step boundary.
///
/// Every request is checked against the model profile's context
/// window; over the threshold, the compact chain rewrites the history
/// the wrapped model serves. Everything else delegates unchanged.
pub(super) struct CompactModel {
    inner: BoxedModel,
    hooks: HookSet,
    agent_name: String,
    model_name: String,
    records: CompactionRecords,
    /// Memoized default-compaction summary, keyed on the summarized
    /// prefix.
    summaries: Mutex<Option<CachedSummary>>,
    /// Cached tools-JSON byte length; the vendor reuses one tools
    /// `Arc` across a run's steps.
    tools_bytes: Mutex<Option<ToolsBytes>>,
}

/// Publication gate for recorded compactions on one event stream.
///
/// Each record anchors to the `ContextInfo` of the step that
/// compacted: the model wrapper and the vendor estimate the same
/// request bytes, so a record's request estimate names its anchor.
/// A record publishes only after its anchor published and another
/// vendor event arrived (the held-back event), so a vendor task
/// running ahead of the consumer cannot reorder events.
pub(super) struct CompactionGate {
    records: CompactionRecords,
    /// Vendor event held back while an anchored record published
    /// first.
    stashed: Option<AgentStreamEvent>,
    /// Token estimate of the last mapped `ContextInfo`.
    anchor: Option<usize>,
}

/// Compaction-held publication due for one stream poll, record
/// first.
pub(super) enum Held {
    /// An anchored compaction record, publishing ahead of the vendor
    /// event the gate holds back.
    Record(RecordedCompaction),
    /// The held-back vendor event, once no anchored record remains.
    Vendor(AgentStreamEvent),
}

/// Recorded compaction outcomes awaiting stream publication.
///
/// The model wrapper records each applied compaction; the
/// `run_stream` wrapper drains the queue as it publishes events.
/// Clones share one queue.
#[derive(Clone, Default)]
pub(crate) struct CompactionRecords {
    queue: Arc<Mutex<VecDeque<RecordedCompaction>>>,
}

/// Identity-keyed cache entry for the serialized tool definitions.
struct ToolsBytes {
    /// Pointer identity of the tool-definition slice.
    ptr: usize,
    /// Tool count when cached.
    len: usize,
    /// Serialized byte length for that identity.
    bytes: usize,
}

/// One applied compaction awaiting stream publication.
///
/// `request_tokens` is the wrapper's token estimate of the request
/// that compacted, which equals the `ContextInfo` estimate the vendor
/// reports for the same request; the stream wrapper anchors
/// publication on it.
pub(super) struct RecordedCompaction {
    /// Token estimate of the request that compacted.
    request_tokens: usize,
    /// The chain's outcome.
    pub(super) result: CompactResult,
}

impl CompactModel {
    /// Wraps `inner` for compaction; the returned records feed the
    /// `run_stream` event wrapper.
    pub(super) fn new(
        inner: BoxedModel,
        hooks: HookSet,
        agent_name: String,
        model_name: String,
    ) -> (Self, CompactionRecords) {
        let records = CompactionRecords::default();
        (
            Self {
                inner,
                hooks,
                agent_name,
                model_name,
                records: records.clone(),
                summaries: Mutex::new(None),
                tools_bytes: Mutex::new(None),
            },
            records,
        )
    }

    /// Returns the serialized byte length of `tools`, cached on the
    /// slice's identity so repeated steps of one run serialize it
    /// once.
    fn cached_tools_bytes(&self, tools: &Arc<Vec<serdes_ai::ToolDefinition>>) -> usize {
        let ptr = Arc::as_ptr(tools) as usize;
        let len = tools.len();
        let mut cached = self
            .tools_bytes
            .lock()
            .expect("tools bytes cache should not be poisoned");
        if let Some(entry) = cached.as_ref()
            && entry.ptr == ptr
            && entry.len == len
        {
            return entry.bytes;
        }
        let bytes = serde_json::to_string(&**tools).map_or(0, |json| json.len());
        *cached = Some(ToolsBytes { ptr, len, bytes });
        bytes
    }

    /// Returns the compacted history when this request overflows and
    /// compaction applies, `None` when it must be served unchanged.
    ///
    /// `None` covers an unknown context window, usage below the
    /// threshold, a cancelled outcome, and any dispatch or
    /// summarization failure: the attempt aborts and the run
    /// continues on the original history.
    async fn compact_request(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Option<Vec<ModelRequest>> {
        let context_limit = self.inner.profile().context_window?;
        let threshold = overflow_threshold(context_limit)?;
        let estimated_tokens =
            history::estimate_request_tokens(messages, self.cached_tools_bytes(&params.tools));
        if u64::try_from(estimated_tokens).unwrap_or(u64::MAX) < threshold {
            return None;
        }
        // Dispatch on every detected overflow, even when nothing is
        // summarizable: hooks keep their agency, and the default
        // executor cancels (history unchanged, no event).
        let entries = history::project_history(messages);
        // Wrapper-generated id: the vendor run id is not visible at
        // the model boundary; tool hooks diverge the same way.
        let run_id = serdes_ai::agent::generate_run_id();
        let ctx = HookRunContext {
            agent_name: &self.agent_name,
            run_id: &run_id,
            model_name: &self.model_name,
        };
        let compactor =
            DefaultCompactor::new(&self.inner, settings, estimated_tokens, &self.summaries);
        // A hook or default failure fails open: serve the original
        // history and record nothing.
        let (outcome, history) = self
            .hooks
            .dispatch_compact(&ctx, entries, &compactor)
            .await
            .ok()?;
        match outcome {
            CompactOutcome::Compacted(result) => {
                let compacted = history::rebuild_history(history);
                self.records.push(estimated_tokens, result);
                Some(compacted)
            }
            CompactOutcome::Cancelled => None,
        }
    }
}

impl CompactionGate {
    /// Opens the gate over this stream's records, dropping outcomes
    /// recorded by earlier runs on this agent.
    pub(super) fn new(records: &CompactionRecords) -> Self {
        records.clear();
        Self {
            records: records.clone(),
            stashed: None,
            anchor: None,
        }
    }

    /// Takes what publishes this poll: an anchored record first, then
    /// the held-back vendor event, `None` when the vendor stream must
    /// be polled.
    pub(super) fn take_held(&mut self) -> Option<Held> {
        if self.stashed.is_some() && self.anchored_pending() {
            self.records.take_pending().map(Held::Record)
        } else {
            self.stashed.take().map(Held::Vendor)
        }
    }

    /// Returns `true` while the oldest record belongs to the last
    /// mapped `ContextInfo`.
    pub(super) fn anchored_pending(&self) -> bool {
        match self.anchor {
            Some(anchor) => self.records.front_matches(anchor),
            None => false,
        }
    }

    /// Holds `event` back: an anchored record publishes first.
    pub(super) fn defer(&mut self, event: AgentStreamEvent) {
        self.stashed = Some(event);
    }

    /// Records the token estimate of a mapped `ContextInfo` as the
    /// publication anchor.
    pub(super) fn track(&mut self, event: &reloaded_code_core::hooks::RunEvent) {
        if let reloaded_code_core::hooks::RunEvent::ContextInfo {
            estimated_tokens, ..
        } = event
        {
            self.anchor = Some(*estimated_tokens);
        }
    }
}

impl CompactionRecords {
    /// Records one applied compaction, dropping the oldest entry past
    /// the cap.
    fn push(&self, request_tokens: usize, result: CompactResult) {
        let mut queue = self.lock();
        if queue.len() >= PENDING_RECORDS_CAP {
            queue.pop_front();
        }
        queue.push_back(RecordedCompaction {
            request_tokens,
            result,
        });
    }

    /// Takes the oldest undelivered record, if any.
    pub(super) fn take_pending(&self) -> Option<RecordedCompaction> {
        self.lock().pop_front()
    }

    /// Returns `true` when the oldest record was recorded for a
    /// request estimating `tokens`: the publication anchor the stream
    /// wrapper tracks.
    pub(super) fn front_matches(&self, tokens: usize) -> bool {
        self.lock()
            .front()
            .is_some_and(|record| record.request_tokens == tokens)
    }

    /// Drops every undelivered record: a new stream must not publish
    /// outcomes recorded before it started.
    pub(super) fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<RecordedCompaction>> {
        self.queue
            .lock()
            .expect("compaction records should not be poisoned")
    }
}

impl RecordedCompaction {
    /// Builds the event for this outcome.
    ///
    /// The event fields come from the chain's result; the record's
    /// own request estimate only anchored the publication position.
    pub(super) fn into_event(self) -> reloaded_code_core::hooks::RunEvent {
        let RecordedCompaction {
            result:
                CompactResult {
                    tokens_before,
                    tokens_after,
                    strategy,
                    messages_before,
                    messages_after,
                    ..
                },
            ..
        } = self;
        reloaded_code_core::hooks::RunEvent::ContextCompressed {
            original_tokens: tokens_before,
            compressed_tokens: tokens_after,
            strategy,
            messages_before,
            messages_after,
        }
    }
}

#[async_trait::async_trait]
impl Model for CompactModel {
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
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<ModelResponse, serdes_ai_models::ModelError> {
        match self.compact_request(messages, settings, params).await {
            Some(compacted) => self.inner.request(&compacted, settings, params).await,
            None => self.inner.request(messages, settings, params).await,
        }
    }

    async fn request_stream(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<serdes_ai_models::StreamedResponse, serdes_ai_models::ModelError> {
        match self.compact_request(messages, settings, params).await {
            Some(compacted) => {
                self.inner
                    .request_stream(&compacted, settings, params)
                    .await
            }
            None => self.inner.request_stream(messages, settings, params).await,
        }
    }

    fn supports(&self, capability: serdes_ai_models::ModelCapability) -> bool {
        self.inner.supports(capability)
    }
}

/// Token threshold that triggers compaction for `context_limit`, or
/// `None` when the window is zero and therefore unusable.
fn overflow_threshold(context_limit: u64) -> Option<u64> {
    (context_limit != 0).then(|| context_limit * OVERFLOW_NUMERATOR / OVERFLOW_DENOMINATOR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::FunctionModel;
    use reloaded_code_core::hooks::{
        CompactHook, CompactHookFuture, CompactMessage, CompactOriginal,
    };
    use serdes_ai::core::{ModelRequestPart, SystemPromptPart, UserContent, UserPromptPart};
    use std::sync::Mutex;

    #[test]
    fn overflow_threshold_maps_window_to_three_quarters() {
        assert_eq!(overflow_threshold(0), None);
        assert_eq!(overflow_threshold(8), Some(6));
        assert_eq!(overflow_threshold(1024), Some(768));
    }

    #[test]
    fn compaction_gate_drops_records_from_earlier_streams() {
        let records = CompactionRecords::default();
        records.push(
            512,
            CompactResult {
                summary: "stale".into(),
                first_kept_entry_id: None,
                tokens_before: 512,
                tokens_after: 64,
                strategy: "summarize".into(),
                messages_before: 6,
                messages_after: 6,
            },
        );
        let _gate = CompactionGate::new(&records);
        assert!(
            records.take_pending().is_none(),
            "a new stream must not publish outcomes recorded before it started"
        );
    }

    /// Compact hook recording each invocation's history length, then
    /// cancelling without calling `original`.
    struct CancellingRecorder {
        invocations: std::sync::Arc<Mutex<Vec<usize>>>,
    }

    impl CompactHook for CancellingRecorder {
        fn hook<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            messages: Vec<CompactMessage>,
            _original: CompactOriginal<'a>,
        ) -> CompactHookFuture<'a> {
            let seen = messages.len();
            self.invocations
                .lock()
                .expect("invocations should not be poisoned")
                .push(seen);
            Box::pin(async move { Ok((CompactOutcome::Cancelled, messages)) })
        }
    }

    #[tokio::test]
    async fn compact_request_dispatches_when_nothing_is_summarizable() {
        // A single oversized prompt overflows a small window but
        // leaves nothing summarizable; the chain still dispatches so
        // registered hooks keep their agency.
        let invocations = std::sync::Arc::new(Mutex::new(Vec::new()));
        let model: BoxedModel = Arc::new(
            FunctionModel::new(|_, _| ModelResponse::text("unused summary"))
                .with_profile(ModelProfile::new().with_context_window(64)),
        );
        let (wrapped, records) = CompactModel::new(
            model,
            HookSet::builder()
                .compact_hook(CancellingRecorder {
                    invocations: Arc::clone(&invocations),
                })
                .build(),
            "coder".into(),
            "gpt-4o".into(),
        );

        let messages = vec![
            ModelRequest::with_parts(vec![ModelRequestPart::SystemPrompt(SystemPromptPart::new(
                "sys",
            ))]),
            ModelRequest::with_parts(vec![ModelRequestPart::UserPrompt(UserPromptPart::new(
                UserContent::Text("x".repeat(400)),
            ))]),
        ];
        let served = wrapped
            .compact_request(
                &messages,
                &ModelSettings::default(),
                &ModelRequestParameters::default(),
            )
            .await;

        assert!(
            served.is_none(),
            "a cancelled outcome must serve the original history"
        );
        assert_eq!(
            *invocations.lock().expect("invocations not poisoned"),
            vec![2],
            "detected overflow must dispatch the chain with the full history"
        );
        assert!(
            records.take_pending().is_none(),
            "a cancelled attempt must record no event"
        );
    }

    #[tokio::test]
    async fn compact_request_fails_open_when_summarization_returns_no_text() {
        // Every summarize request answers without text, so the
        // default compaction fails and the attempt must abort.
        let model: BoxedModel = Arc::new(
            FunctionModel::new(|_, _| ModelResponse::with_parts(vec![]))
                .with_profile(ModelProfile::new().with_context_window(64)),
        );
        let (wrapped, records) = CompactModel::new(
            model,
            HookSet::builder().build(),
            "coder".into(),
            "gpt-4o".into(),
        );

        let mut messages = vec![ModelRequest::with_parts(vec![
            ModelRequestPart::SystemPrompt(SystemPromptPart::new("sys")),
        ])];
        for _ in 0..6 {
            messages.push(ModelRequest::with_parts(vec![
                ModelRequestPart::UserPrompt(UserPromptPart::new(UserContent::Text(
                    "x".repeat(200),
                ))),
            ]));
        }
        let served = wrapped
            .compact_request(
                &messages,
                &ModelSettings::default(),
                &ModelRequestParameters::default(),
            )
            .await;
        assert!(
            served.is_none(),
            "a failed summarization must leave the request unchanged"
        );
        assert!(
            records.take_pending().is_none(),
            "a failed attempt must record no event"
        );
    }
}
