#![warn(missing_docs)]
//! Core types and utilities for coding tools.
//!
//! This crate provides framework-agnostic building blocks:
//! - [`ToolError`] and [`ToolResult`] for error handling
//! - [`ToolOutput`] for tool responses with truncation metadata
//! - Utility functions for text processing
//!
//! # Features
//!
//! - `async`: Enables async function signatures and async-only modules.
//! - `tokio` (default): Enables async via tokio runtime (implies `async`).
//!   When disabled, all operations are synchronous.

// Validate feature combinations at compile time
#[cfg(all(feature = "async", not(feature = "tokio")))]
compile_error!("Feature `async` requires a runtime. Enable `tokio` feature instead.");

#[cfg(all(feature = "async", feature = "blocking"))]
compile_error!("Features `async` and `blocking` are mutually exclusive.");

pub mod context;
pub mod error;
pub mod fs;
pub mod operations;
pub mod output;
pub mod path;
pub mod preamble;
pub mod tool_names;
pub mod util;

pub use context::ToolContext;
pub use error::{ToolError, ToolResult};
pub use output::ToolOutput;
pub use path::{AbsolutePathResolver, AllowedPathResolver, PathResolver};
pub use preamble::{PreambleBuilder, Substitute};

// Re-export operations (always available, sync or async based on runtime feature)
pub use operations::{
    edit_file, execute_command, glob_files, grep_search, read_file, read_todos, write_file,
    write_todos, BashOutput, EditError, GlobOutput, GrepFileMatches, GrepLineMatch, GrepOutput,
    Todo, TodoPriority, TodoState, TodoStatus,
};

// Re-export webfetch operations (requires async or blocking feature)
#[cfg(any(feature = "async", feature = "blocking"))]
pub use operations::{fetch_url, format_json, html_to_markdown, WebFetchOutput};

// Re-export async-only operations (requires async feature)
#[cfg(feature = "async")]
pub use operations::{MockTaskExecutor, TaskArgs, TaskExecutor, TaskResult};
