//! Tool adapters for the serdes_ai tool framework.
//!
//! Each tool wraps a core operation and adapts it to the [`Tool`] trait
//! from `serdes_ai`.
//!
//! File tools use a [`PathResolver`] to validate and resolve paths. The
//! path mode (absolute or sandboxed) is detected automatically from the
//! resolver type at construction time, which selects the correct schema
//! parameter names and descriptions.
//!
//! Supported path resolvers:
//! - [`AbsolutePathResolver`] - unrestricted absolute path access
//! - [`AllowedPathResolver`] - sandboxed directory-restricted access
//!
//! # Public API
//!
//! File tools (generic over [`PathResolver`]):
//! - [`EditTool`] - exact string replacement in files
//! - [`GlobTool`] - find files matching glob patterns
//! - [`GrepTool`] - search file contents with regex patterns
//! - [`ReadTool`] - read file contents with optional line ranges
//! - [`WriteTool`] - write content to files
//!
//! Shell tools:
//! - [`BashTool`] - execute shell commands on the host or in a sandbox
//!
//! Web tools:
//! - [`WebFetchTool`] - fetch and convert web content from URLs
//!
//! Task management:
//! - [`TodoReadTool`] - read the current todo list
//! - [`TodoWriteTool`] - write/replace the todo list
//! - [`create_todo_tools`] - create a linked read/write pair with shared state
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_serdesai::{ReadTool, AbsolutePathResolver};
//!
//! let read_tool = ReadTool::new(AbsolutePathResolver);
//! ```
//!
//! [`PathResolver`]: llm_coding_tools_core::path::PathResolver
//! [`AbsolutePathResolver`]: llm_coding_tools_core::path::AbsolutePathResolver
//! [`AllowedPathResolver`]: llm_coding_tools_core::path::AllowedPathResolver
//! [`Tool`]: serdes_ai::tools::Tool

mod bash;
mod edit;
mod glob;
mod grep;
mod read;
pub mod todo;
mod webfetch;
mod write;

pub use bash::BashTool;
pub use edit::EditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use read::ReadTool;
pub use todo::{TodoReadTool, TodoWriteTool, create_todo_tools};
pub use webfetch::WebFetchTool;
pub use write::WriteTool;
