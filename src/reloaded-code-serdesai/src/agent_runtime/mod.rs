//! SerdesAI adapter for the generic agent runtime.
//!
//! The data-only runtime foundation lives in `reloaded-code-agents`. This
//! module re-exports agent runtime types and adds SerdesAI-specific build
//! orchestration through [`AgentBuildContext`].
//!
//! # Public API
//! - [`AgentBuildContext`] - Shared context that builds runnable agents by name.
//! - [`AgentBuildError`] - Build-time failures.

mod build;
mod model;
mod provider_bridge;
mod task;
#[cfg(test)]
mod test_stubs;

pub use build::AgentBuildError;
pub use reloaded_code_agents::{
    AgentDefaults, AgentRuntime, AgentRuntimeBuilder, ModelResolutionError, ResolvedModel,
    resolve_model_with_catalog,
};
pub use task::AgentBuildContext;
pub(crate) use task::{TaskBuildContext, build_agent};
