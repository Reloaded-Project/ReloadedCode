#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]
#![warn(missing_docs)]

/// Re-export preferred Linux bubblewrap profile types
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
pub use reloaded_code_bubblewrap::profile;
/// Re-export [`SystemPromptBuilder`] from core.
pub use reloaded_code_core::SystemPromptBuilder;
/// Re-export context module and [`ToolContext`] trait for convenience.
pub use reloaded_code_core::ToolContext;
pub use reloaded_code_core::context;
/// Re-export path resolvers from core.
pub use reloaded_code_core::path::{
    AbsolutePathResolver, AllowedGlobResolver, AllowedPathResolver, PathResolver,
};
/// Re-export bash execution mode and mode-aware execution.
pub use reloaded_code_core::{BashExecutionMode, execute_command_with_mode};
/// Re-export core types for convenience.
pub use reloaded_code_core::{TaskSettings, ToolError, ToolOutput, ToolResult};
// Re-export tools from the tools module
pub use tools::{
    BashTool, CustomToolAdapter, EditTool, GlobTool, GrepTool, ReadTool, TodoReadTool,
    TodoWriteTool, WebFetchTool, WriteTool, create_todo_tools,
};
// Re-export core operation types used by tools
pub use reloaded_code_core::{
    BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput, Todo,
    TodoPriority, TodoState, TodoStatus, WebFetchOutput,
};
// Re-export standalone tools and runtime helpers
pub use agent_runtime::{AgentBuildContext, AgentBuildError, HookedAgent, HookedAgentRunResult};
pub use reloaded_code_agents::{
    AgentDefaults, AgentRuntime, AgentRuntimeBuilder, ModelResolutionError, ResolvedModel,
    resolve_model_with_catalog,
};
/// Re-export [`RunEvent`], the framework-owned item type yielded by
/// [`HookedAgent::run_stream`], together with its transcript payload
/// types ([`RunMessage`], [`RunMessageRole`], [`RunToolCallSummary`],
/// [`RunToolResultSummary`]), and [`RunEventHook`], the hook that
/// intercepts each streamed event before publication.
pub use reloaded_code_core::hooks::{
    RunEvent, RunEventHook, RunMessage, RunMessageRole, RunToolCallSummary, RunToolResultSummary,
};

pub mod agent_ext;
pub mod agent_runtime;
pub mod convert;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod task;
pub mod tools;
