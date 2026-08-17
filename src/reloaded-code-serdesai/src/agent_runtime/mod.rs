//! SerdesAI adapter for the generic agent runtime.
//!
//! The data-only runtime foundation lives in `reloaded-code-agents`. This
//! module re-exports agent runtime types and adds SerdesAI-specific build
//! orchestration through [`AgentBuildContext`].
//!
//! # Public API
//! - [`AgentBuildContext`] - Shared context that builds runnable agents by name.
//! - [`HookedAgent`] - Built agent wrapper that dispatches `run()` through run
//!   hooks and streams framework-owned events from `run_stream()`, passing
//!   each through the registered run-event hooks.
//! - [`AgentBuildError`] - Build-time failures.

pub use build::AgentBuildError;
pub use reloaded_code_agents::{
    AgentDefaults, AgentRuntime, AgentRuntimeBuilder, ModelResolutionError, ResolvedModel,
    resolve_model_with_catalog,
};
pub use task::{AgentBuildContext, HookedAgent, HookedAgentRunResult};
pub(crate) use task::{TaskBuildContext, build_agent};

mod build;
mod model;
mod provider_bridge;
mod stream_events;
mod task;
#[cfg(test)]
pub(crate) mod test_stubs;
