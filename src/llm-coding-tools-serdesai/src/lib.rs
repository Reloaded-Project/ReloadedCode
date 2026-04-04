#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]
#![warn(missing_docs)]

pub mod agent_ext;
pub mod agent_runtime;
pub mod convert;
pub mod task;
pub mod tools;

/// Re-export core types for convenience.
pub use llm_coding_tools_core::{ToolError, ToolOutput, ToolResult};

/// Re-export bash execution mode and mode-aware execution.
pub use llm_coding_tools_core::{BashExecutionMode, execute_command_with_mode};

/// Re-export preferred Linux bubblewrap profile types
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
pub use llm_coding_tools_bubblewrap::profile;

/// Re-export context module and [`ToolContext`] trait for convenience.
pub use llm_coding_tools_core::ToolContext;
pub use llm_coding_tools_core::context;

/// Re-export [`SystemPromptBuilder`] from core.
pub use llm_coding_tools_core::SystemPromptBuilder;

/// Re-export path resolvers from core.
pub use llm_coding_tools_core::path::{AbsolutePathResolver, AllowedPathResolver, PathResolver};

// Re-export tools from the tools module
pub use tools::{
    BashTool, EditTool, GlobTool, GrepTool, ReadTool, TodoReadTool, TodoWriteTool, WebFetchTool,
    WriteTool, create_todo_tools,
};

// Re-export core operation types used by tools
pub use llm_coding_tools_core::{
    BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput, Todo,
    TodoPriority, TodoState, TodoStatus, WebFetchOutput,
};

// Re-export standalone tools and runtime helpers
pub use agent_runtime::{AgentBuildContext, AgentBuildError};
pub use llm_coding_tools_agents::{
    AgentDefaults, AgentRuntime, AgentRuntimeBuilder, ModelResolutionError, ResolvedModel,
    TaskSettings, ToolCatalogEntry, ToolCatalogKind, default_tools, resolve_model_with_catalog,
};
