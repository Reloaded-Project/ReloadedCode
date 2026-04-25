#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

// Validate feature combinations at compile time
#[cfg(all(feature = "async", feature = "blocking"))]
compile_error!("Features `async` and `blocking` are mutually exclusive.");

#[cfg(not(any(feature = "async", feature = "blocking")))]
compile_error!("Either an async runtime (e.g., `tokio`) or `blocking` feature must be enabled.");

pub mod context;
pub mod credentials;
pub mod error;
pub mod fs;
pub mod models;
pub mod output;
pub mod path;
pub mod permissions;
pub mod permissions_ext;
pub mod system_prompt;
pub mod tool_metadata;
pub mod tools;
pub mod util;
pub mod workspace;

mod internal;

pub use context::ToolContext;
pub use credentials::{CredentialLookup, CredentialResolver};
pub use error::{ToolError, ToolResult};
pub use output::ToolOutput;
pub use path::{AbsolutePathResolver, AllowedGlobResolver, AllowedPathResolver, PathResolver};
pub use system_prompt::SystemPromptBuilder;
pub use workspace::resolve_workspace_root;

// Re-export tools (always available, sync or async based on runtime feature)
pub use tools::{
    edit_file, execute_command, execute_command_with_mode, glob_files, grep_search, read_file,
    read_todos, write_file, write_todos, BashExecutionMode, BashOutput, BashRequest, BashSettings,
    EditError, EditRequest, EditSettings, GlobOutput, GlobRequest, GlobSettings, GrepFileMatches,
    GrepFormattingSettings, GrepLineMatch, GrepOutput, GrepRequest, GrepSettings, ReadRequest,
    ReadSettings, TaskInput, TaskOutput, TaskSettings, Todo, TodoPriority, TodoReadRequest,
    TodoState, TodoStatus, TodoWriteRequest, WriteRequest, WriteSettings,
};

// Re-export Linux sandbox types (Linux-only, requires linux-bubblewrap feature)
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
pub use tools::linux_bwrap_profile;

// Re-export webfetch tools (requires tokio or blocking feature)
#[cfg(any(feature = "tokio", feature = "blocking"))]
pub use tools::{
    fetch_url, format_json, html_to_markdown, WebFetchOutput, WebFetchRequest, WebFetchSettings,
};
