#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]
#![warn(missing_docs)]

// Validate feature combinations at compile time
#[cfg(all(feature = "async", feature = "blocking"))]
compile_error!("Features `async` and `blocking` are mutually exclusive.");

#[cfg(not(any(feature = "async", feature = "blocking")))]
compile_error!("Either an async runtime (e.g., `tokio`) or `blocking` feature must be enabled.");

pub mod context;
pub mod error;
pub mod fs;
pub mod output;
pub mod path;
pub mod permissions;
pub mod system_prompt;
pub mod tool_names;
pub mod tools;
pub mod util;

pub use context::ToolContext;
pub use error::{ToolError, ToolResult};
pub use output::ToolOutput;
pub use path::{AbsolutePathResolver, AllowedPathResolver, PathResolver};
pub use system_prompt::{Substitute, SystemPromptBuilder};

// Re-export tools (always available, sync or async based on runtime feature)
pub use tools::{
    edit_file, execute_command, glob_files, grep_search, read_file, read_todos, write_file,
    write_todos, BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput,
    TaskInput, TaskOutput, Todo, TodoPriority, TodoState, TodoStatus,
};

// Re-export webfetch tools (requires async or blocking feature)
#[cfg(any(feature = "async", feature = "blocking"))]
pub use tools::{fetch_url, format_json, html_to_markdown, WebFetchOutput};
