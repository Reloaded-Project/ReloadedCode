//! Step-boundary context compaction for the SerdesAI run loop.
//!
//! # Wiring
//!
//! Both vendor run paths call the model once per step with the whole
//! conversation, so the model boundary is the public seam where the
//! wrapper sees each step's history. When the build enables
//! compaction, `build_agent` wraps the resolved model in
//! [`CompactModel`]; a build that leaves compaction disabled keeps
//! its model untouched and nothing here runs.
//!
//! Each model request estimates context usage with the same heuristic
//! the vendor's `ContextInfo` uses: serialized request bytes over
//! four, against the model profile's context window. Past the policy
//! threshold, the request projects onto [`CompactEntry`] views and
//! the Core [`Compactor`] summarizes the older entries through the
//! run's own model, keeping a recent window verbatim. The compacted
//! history becomes the request the model serves, which applies the
//! outcome without touching vendor internals.
//!
//! # Events and failure
//!
//! Each applied compaction queues its [`CompactionRecord`]; the
//! `run_stream` wrapper publishes queued records as
//! [`RunEvent::ContextCompressed`] once their step's `ContextInfo`
//! has published. The `run()` path keeps compaction internal by
//! design: history effect only, no event surface.
//!
//! A failed or empty summarization aborts the attempt: the original
//! history is served unchanged and no event is queued, so the run
//! continues.
//!
//! # Remarks
//!
//! One record queue and one summary cache are shared by every run of
//! the agent, so concurrent runs through one agent get best-effort
//! attribution.
//!
//! Next: see [`CompactPolicy`] for the trigger and cap defaults.
//!
//! [`CompactEntry`]: reloaded_code_core::CompactEntry
//! [`Compactor`]: reloaded_code_core::Compactor
//! [`CompactionRecord`]: reloaded_code_core::CompactionRecord
//! [`CompactPolicy`]: reloaded_code_core::CompactPolicy
//! [`RunEvent::ContextCompressed`]: reloaded_code_core::hooks::RunEvent::ContextCompressed

use executor::ModelSummaryExecutor;
use projection::{project_history, rebuild_history};
use reloaded_code_core::hooks::RunEvent;
use reloaded_code_core::{CompactPolicy, CompactionRecord, Compactor};
use serdes_ai::AgentStreamEvent;
use serdes_ai::core::{ModelRequest, ModelResponse, ModelSettings};
use serdes_ai_models::{BoxedModel, Model, ModelProfile, ModelRequestParameters};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

mod executor;
mod projection;
#[cfg(test)]
mod tests;

/// Bound on undelivered compaction records; the oldest drop first.
/// One record per overflowing step keeps realistic streams far below
/// this, and `run()` calls, which drain nothing, stay bounded.
const PENDING_RECORDS_CAP: usize = 1024;

/// Model wrapper running context compaction at each step boundary.
///
/// Every request is checked against the model profile's context
/// window; past the policy threshold, the Core [`Compactor`] rewrites
/// the history the wrapped model serves. Everything else delegates
/// unchanged.
///
/// [`Compactor`]: reloaded_code_core::Compactor
pub(super) struct CompactModel {
    /// Wrapped model serving every request and summarize call.
    inner: BoxedModel,
    /// Trigger threshold policy applied per request.
    policy: CompactPolicy,
    /// Core planner owning the summary cache and the summarize cap.
    compactor: Compactor,
    /// Applied compactions awaiting stream publication.
    records: CompactionRecords,
    /// Cached tools-JSON byte length; the vendor reuses one tools
    /// `Arc` across a run's steps.
    tools_bytes: Mutex<Option<ToolsBytes>>,
}

/// Publication gate for queued compaction records on one event
/// stream.
///
/// Each record anchors to the `ContextInfo` of the step that
/// compacted: the model wrapper and the vendor estimate the same
/// request bytes, so a record's request estimate names its anchor.
/// A record publishes only after its anchor published and another
/// vendor event arrived (the held-back event), so a vendor task
/// running ahead of the consumer cannot reorder events.
pub(super) struct CompactionGate {
    /// Records this stream may publish.
    records: CompactionRecords,
    /// Vendor event held back while an anchored record published
    /// first.
    stashed: Option<AgentStreamEvent>,
    /// Token estimate of the last mapped `ContextInfo`.
    anchor: Option<usize>,
}

/// Sink counting the bytes a serializer writes.
struct CountingWriter(usize);

/// Compaction-held publication due for one stream poll, record first.
pub(super) enum Held {
    /// An anchored compaction record as its published event, ahead of
    /// the vendor event the gate holds back.
    Record(RunEvent),
    /// The held-back vendor event, once no anchored record remains.
    Vendor(AgentStreamEvent),
}

/// Applied compactions awaiting stream publication.
///
/// The model wrapper queues each applied compaction; the `run_stream`
/// wrapper drains the queue as it publishes events. Clones share one
/// queue.
#[derive(Clone, Default)]
pub(crate) struct CompactionRecords {
    queue: Arc<Mutex<VecDeque<CompactionRecord>>>,
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

impl CompactModel {
    /// Wraps `inner` for compaction under `policy`; the returned
    /// records feed the `run_stream` event wrapper.
    ///
    /// The model profile's advertised `max_tokens`, when known,
    /// clamps the policy's summarize cap.
    pub(super) fn new(inner: BoxedModel, policy: CompactPolicy) -> (Self, CompactionRecords) {
        let compactor = Compactor::new(policy, inner.profile().max_tokens);
        let records = CompactionRecords::default();
        (
            Self {
                inner,
                policy,
                compactor,
                records: records.clone(),
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

    /// Returns the compacted history when this request crosses the
    /// policy threshold and compaction applies, `None` when it must
    /// be served unchanged.
    ///
    /// `None` covers an unknown context window, usage below the
    /// threshold, nothing summarizable, and any summarization
    /// failure: the attempt aborts and the run continues on the
    /// original history.
    async fn compact_request(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Option<Vec<ModelRequest>> {
        let context_limit = self.inner.profile().context_window;
        let estimated_tokens =
            estimate_request_tokens(messages, self.cached_tools_bytes(&params.tools));
        let tokens = u64::try_from(estimated_tokens).unwrap_or(u64::MAX);
        if !self.policy.should_compact(context_limit, tokens) {
            return None;
        }
        // Fail-open: a failed or empty summarization serves the
        // original history and queues no event.
        let attempt = self
            .compactor
            .compact(
                &ModelSummaryExecutor::new(&self.inner, settings),
                project_history(messages),
                estimated_tokens,
            )
            .await
            .ok()
            .flatten()?;
        let compacted = rebuild_history(attempt.history);
        self.records.push(attempt.record);
        Some(compacted)
    }
}

impl CompactionGate {
    /// Opens the gate over this stream's records, dropping outcomes
    /// queued by earlier runs on this agent.
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
            self.records
                .take_pending()
                .map(|record| Held::Record(context_compressed_event(record)))
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
    pub(super) fn track(&mut self, event: &RunEvent) {
        if let RunEvent::ContextInfo {
            estimated_tokens, ..
        } = event
        {
            self.anchor = Some(*estimated_tokens);
        }
    }
}

impl CompactionRecords {
    /// Queues one applied compaction, dropping the oldest entry past
    /// the cap.
    fn push(&self, record: CompactionRecord) {
        let mut queue = self.lock();
        if queue.len() >= PENDING_RECORDS_CAP {
            queue.pop_front();
        }
        queue.push_back(record);
    }

    /// Takes the oldest undelivered record, if any.
    fn take_pending(&self) -> Option<CompactionRecord> {
        self.lock().pop_front()
    }

    /// Returns `true` when the oldest record was queued for a request
    /// estimating `tokens`: the publication anchor the stream wrapper
    /// tracks.
    fn front_matches(&self, tokens: usize) -> bool {
        self.lock()
            .front()
            .is_some_and(|record| record.tokens_before == tokens)
    }

    /// Drops every undelivered record: a new stream must not publish
    /// outcomes queued before it started.
    fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> MutexGuard<'_, VecDeque<CompactionRecord>> {
        self.queue
            .lock()
            .expect("compaction records should not be poisoned")
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

    async fn count_tokens(
        &self,
        messages: &[ModelRequest],
    ) -> Result<u64, serdes_ai_models::ModelError> {
        self.inner.count_tokens(messages).await
    }
}

impl std::io::Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Builds the published event for one applied compaction.
///
/// The record's request-level estimate becomes the event's
/// `original_tokens`; the record's summary has no event counterpart.
fn context_compressed_event(record: CompactionRecord) -> RunEvent {
    let CompactionRecord {
        tokens_before,
        tokens_after,
        strategy,
        messages_before,
        messages_after,
        ..
    } = record;
    RunEvent::ContextCompressed {
        original_tokens: tokens_before,
        compressed_tokens: tokens_after,
        strategy,
        messages_before,
        messages_after,
    }
}

/// Estimated tokens of one model request, mirroring the vendor's
/// `ContextInfo` heuristic: serialized message and tool bytes over
/// four. `tools_bytes` is the caller-cached serialized length of the
/// request's tool definitions. Counted, not materialized: only the
/// byte total is needed, so no conversation-sized buffer is built.
fn estimate_request_tokens(messages: &[ModelRequest], tools_bytes: usize) -> usize {
    let mut writer = CountingWriter(0);
    let messages_bytes = serde_json::to_writer(&mut writer, messages).map_or(0, |()| writer.0);
    (messages_bytes + tools_bytes) / 4
}
