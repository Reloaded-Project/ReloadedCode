//! Hook run lifecycle event types.

/// Why a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Run completed normally.
    Completed,
    /// Run was stopped externally.
    Stopped,
    /// Run failed (LLM error, length limit, content filter).
    Failed,
}

/// Context given to hook run lifecycle events.
#[derive(Debug)]
pub struct HookRunContext<'a> {
    /// Name of the agent running the hook.
    pub agent_name: &'a str,
    /// Unique identifier for the current run.
    pub run_id: &'a str,
    /// Name of the model being used for this run.
    pub model_name: &'a str,
}

/// Compact event callback. Name preserved - compact is its own concept, distinct from "run".
pub type SessionCompactFn = for<'a> fn(&'a HookRunContext<'a>);
