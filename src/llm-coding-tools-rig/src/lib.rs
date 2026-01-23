#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]
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

// Re-export SystemPromptBuilder and Substitute from core
pub use llm_coding_tools_core::{Substitute, SystemPromptBuilder};

// Re-export path resolvers
pub use llm_coding_tools_core::path::{AbsolutePathResolver, AllowedPathResolver, PathResolver};

// Re-export core operation types used by tools
pub use llm_coding_tools_core::{
    BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput, Todo,
    TodoPriority, TodoState, TodoStatus, WebFetchOutput,
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
    use llm_coding_tools_core::tool_names;

    #[test]
    fn system_prompt_builder_with_real_tools() {
        let mut pb = SystemPromptBuilder::new();
        let read: absolute::ReadTool<true> = pb.track(absolute::ReadTool::new());
        let bash = pb.track(BashTool::new());

        let prompt = pb.build();

        assert!(prompt.contains("## `Read` Tool"));
        assert!(prompt.contains("## `Bash` Tool"));
        assert!(prompt.contains("absolute path")); // From READ_ABSOLUTE

        // Tools are returned unchanged
        assert_eq!(
            <absolute::ReadTool<true> as rig::tool::Tool>::NAME,
            tool_names::READ
        );
        let _ = read;
        let _ = bash;
    }
}
