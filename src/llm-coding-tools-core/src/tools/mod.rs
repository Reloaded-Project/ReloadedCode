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

pub use bash::{execute_command, BashOutput};
pub use edit::{edit_file, EditError};
pub use glob::{glob_files, GlobOutput};
pub use grep::{grep_search, GrepFileMatches, GrepLineMatch, GrepOutput, DEFAULT_MAX_LINE_LENGTH};
pub use read::read_file;
pub use task::{TaskInput, TaskOutput};
pub use todo::{read_todos, write_todos, Todo, TodoPriority, TodoState, TodoStatus};
pub use write::write_file;

// Webfetch available in both async and blocking modes
#[cfg(any(feature = "async", feature = "blocking"))]
pub mod webfetch;

#[cfg(any(feature = "async", feature = "blocking"))]
pub use webfetch::{fetch_url, format_json, html_to_markdown, WebFetchOutput};
