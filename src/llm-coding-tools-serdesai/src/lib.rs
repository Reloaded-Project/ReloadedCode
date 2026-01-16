//! serdesAI framework Tool implementations for coding tools.
//!
//! This crate provides `serdes_ai::Tool` implementations wrapping
//! the core operations from [`llm_coding_tools_core`].
//!
//! # Module Organization
//!
//! - [`absolute`] - Tools requiring absolute paths (no path restriction)
//! - [`allowed`] - Tools restricted to allowed directories
//! - Standalone tools (bash, task, todo, webfetch) at crate root
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_serdesai::absolute::ReadTool;
//! use llm_coding_tools_serdesai::BashTool;
//! ```

#![warn(missing_docs)]

pub mod absolute;
pub mod allowed;
pub mod bash;
pub mod convert;
pub mod task;
pub mod todo;
pub mod webfetch;

pub(crate) mod schema;

/// Re-export core types for convenience.
pub use llm_coding_tools_core::{ToolError, ToolOutput, ToolResult};

/// Re-export context module and [`ToolContext`] trait for convenience.
pub use llm_coding_tools_core::ToolContext;
pub use llm_coding_tools_core::context;

/// Re-export [`PreambleBuilder`] and [`Substitute`] from core.
pub use llm_coding_tools_core::{PreambleBuilder, Substitute};

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
    BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput,
    MockTaskExecutor, TaskExecutor, TaskResult, Todo, TodoPriority, TodoState, TodoStatus,
    WebFetchOutput,
};

// Re-export conversion utilities
pub use convert::{edit_error_to_serdes, output_to_return, to_serdes_result};

// Re-export standalone tools
pub use bash::BashTool;
pub use task::TaskTool;
pub use todo::{TodoReadTool, TodoWriteTool, create_todo_tools};
pub use webfetch::WebFetchTool;
