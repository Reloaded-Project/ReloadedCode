#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

pub mod absolute;
pub mod agent_ext;
pub mod allowed;
pub mod bash;
mod common;
pub mod convert;
/// Model resolver for resolving model specs into provider-specific settings.
pub mod model_resolver;
pub mod registry;
pub mod task;
pub mod todo;
pub mod tool_catalog;
pub mod webfetch;

/// Re-export core types for convenience.
pub use llm_coding_tools_core::{ToolError, ToolOutput, ToolResult};

/// Re-export context module and [`ToolContext`] trait for convenience.
pub use llm_coding_tools_core::ToolContext;
pub use llm_coding_tools_core::context;

/// Re-export [`SystemPromptBuilder`] from core.
pub use llm_coding_tools_core::SystemPromptBuilder;

/// Re-export path resolvers from core.
pub use llm_coding_tools_core::path::{AbsolutePathResolver, AllowedPathResolver, PathResolver};

// Re-export absolute path tools
pub use absolute::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};

/// Re-export allowed module tool types (namespaced to avoid conflicts).
///
/// Use this module when you need both absolute and allowed tools:
///
/// ```no_run
/// use llm_coding_tools_serdesai::{ReadTool, WriteTool};  // absolute
/// use llm_coding_tools_serdesai::allowed_tools::{ReadTool as SandboxedReadTool};
/// ```
pub mod allowed_tools {
    pub use crate::allowed::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};
}

// Re-export core operation types used by tools
pub use llm_coding_tools_core::{
    BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput, Todo,
    TodoPriority, TodoState, TodoStatus, WebFetchOutput,
};

// Re-export standalone tools
pub use bash::BashTool;
pub use model_resolver::{
    ModelResolveError, ModelResolver, ModelsDevResolver, ProviderOverride, ProviderOverrides,
    ResolutionSource, ResolvedModel,
};
pub use registry::{
    AgentDefaults, AgentRegistry, AgentRegistryBuildError, AgentRegistryBuilder,
    AgentRegistryEntry, RegistryAgent, RegistryAgentError,
};
pub use task::{TaskDefinitionSnapshot, TaskRegistryHandle, TaskTargetSummary, TaskTool};
pub use todo::{TodoReadTool, TodoWriteTool, create_todo_tools};
pub use tool_catalog::{ToolCatalogEntry, default_tools};
pub use webfetch::WebFetchTool;
