//! Tool adapters for the serdes_ai tool framework.
//!
//! Built-in tools wrap core operations and adapt them to the [`Tool`] trait
//! from `serdes_ai`.
//!
//! [`CustomToolAdapter`] wraps a portable custom tool so user-defined tools
//! can be attached without writing a SerdesAI-specific wrapper.
//!
//! File tools use a [`PathResolver`] to validate and resolve paths. The
//! path mode (absolute or sandboxed) is detected automatically from the
//! resolver type at construction time, which selects the correct schema
//! parameter names and descriptions.
//!
//! These tools work with any [`PathResolver`] implementation:
//! - [`AbsolutePathResolver`] - unrestricted absolute path access
//! - [`AllowedPathResolver`] - sandboxed directory-restricted access
//! - [`AllowedGlobResolver`] for sandboxed access with glob pattern filtering
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
//! Adapter tools:
//! - [`CustomToolAdapter`] - wrap a portable [`reloaded_code_core::CustomTool`]
//!
//! # Example
//!
//! ```no_run
//! use reloaded_code_serdesai::{ReadTool, AbsolutePathResolver};
//!
//! let read_tool = ReadTool::new(AbsolutePathResolver);
//! ```
//!
//! [`PathResolver`]: reloaded_code_core::path::PathResolver
//! [`AbsolutePathResolver`]: reloaded_code_core::path::AbsolutePathResolver
//! [`AllowedPathResolver`]: reloaded_code_core::path::AllowedPathResolver
//! [`Tool`]: serdes_ai::tools::Tool
//! [`AllowedGlobResolver`]: reloaded_code_core::path::AllowedGlobResolver

mod bash;
mod custom;
mod edit;
mod glob;
mod grep;
mod read;
pub mod todo;
mod webfetch;
mod write;

pub use bash::BashTool;
pub use custom::CustomToolAdapter;
pub use edit::EditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use read::ReadTool;
pub use todo::{TodoReadTool, TodoWriteTool, create_todo_tools};
pub use webfetch::WebFetchTool;
pub use write::WriteTool;
