//! Rig framework Tool implementations for coding tools.
//!
//! This crate provides `rig_core::tool::Tool` implementations wrapping
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
//! use llm_coding_tools_rig::absolute::ReadTool;
//! use llm_coding_tools_rig::BashTool;
//! ```

#![warn(missing_docs)]

pub mod absolute;
pub mod allowed;
pub mod bash;
pub mod task;
pub mod todo;
pub mod webfetch;

// Re-export core types for convenience
pub use llm_coding_tools_core::{ToolError, ToolOutput, ToolResult};

// Re-export context module and ToolContext trait for convenience
pub use llm_coding_tools_core::context;
pub use llm_coding_tools_core::ToolContext;

// Re-export PreambleBuilder and Substitute from core
pub use llm_coding_tools_core::{PreambleBuilder, Substitute};

// Re-export path resolvers
pub use llm_coding_tools_core::path::{AbsolutePathResolver, AllowedPathResolver, PathResolver};

// Re-export core operation types used by tools
pub use llm_coding_tools_core::{
    BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput,
    MockTaskExecutor, TaskExecutor, TaskResult, Todo, TodoPriority, TodoState, TodoStatus,
    WebFetchOutput,
};

// Re-export absolute module tool types
pub use absolute::{
    EditArgs, EditTool, GlobArgs, GlobTool, GrepArgs, GrepTool, ReadArgs, ReadTool, WriteTool,
    WriteToolArgs,
};

/// Re-export allowed module tool types (namespaced to avoid conflicts)
pub mod allowed_tools {
    pub use crate::allowed::{
        EditArgs, EditError, EditTool, GlobArgs, GlobTool, GrepArgs, GrepTool, ReadArgs, ReadTool,
        WriteTool, WriteToolArgs,
    };
}

// Re-export standalone tools
pub use bash::{BashArgs, BashTool};
pub use task::{TaskArgs, TaskTool};
pub use todo::{TodoReadArgs, TodoReadTool, TodoTools, TodoWriteArgs, TodoWriteTool};
pub use webfetch::{WebFetchArgs, WebFetchTool};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_builder_with_real_tools() {
        let mut pb = PreambleBuilder::<false>::new();
        let read: absolute::ReadTool<true> = pb.track(absolute::ReadTool::new());
        let bash = pb.track(BashTool::new());

        let preamble = pb.build();

        assert!(preamble.contains("## Read Tool"));
        assert!(preamble.contains("## Bash Tool"));
        assert!(preamble.contains("absolute path")); // From READ_ABSOLUTE

        // Tools are returned unchanged
        assert_eq!(<absolute::ReadTool<true> as rig::tool::Tool>::NAME, "Read");
        let _ = read;
        let _ = bash;
    }
}
