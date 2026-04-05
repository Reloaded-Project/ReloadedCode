//! Core tools for file systems and utilities.
//!
//! This module contains framework-agnostic implementations of:
//! - File tools (read, write, edit, glob, grep, bash, todo) - always available
//! - Web fetching (fetch_url) - requires `async` or `blocking` feature

// Always available (sync or async based on runtime feature)
pub mod bash;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod read;
pub mod task;
pub mod todo;
pub mod write;

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
pub use bash::linux_bwrap_profile;
pub use bash::{
    execute_command, execute_command_with_mode, BashExecutionMode, BashOutput, BashRequest,
    BashSettings,
};
pub use edit::{edit_file, EditError, EditRequest};
pub use glob::{glob_files, GlobOutput, GlobRequest, GlobSettings};
pub use grep::{
    grep_search, GrepFileMatches, GrepFormattingSettings, GrepLineMatch, GrepOutput, GrepRequest,
    GrepSettings, DEFAULT_MAX_LINE_LENGTH,
};
pub use read::{read_file, ReadRequest, ReadSettings};
pub use task::{TaskInput, TaskOutput, TaskSettings};
pub use todo::{
    read_todos, write_todos, Todo, TodoPriority, TodoReadRequest, TodoState, TodoStatus,
    TodoWriteRequest,
};
pub use write::{write_file, WriteRequest};

// Webfetch available in both tokio and blocking modes
#[cfg(any(feature = "tokio", feature = "blocking"))]
pub mod webfetch;

#[cfg(any(feature = "tokio", feature = "blocking"))]
pub use webfetch::{
    fetch_url, format_json, html_to_markdown, WebFetchOutput, WebFetchRequest, WebFetchSettings,
};
