//! Tools using [`llm_coding_tools_core::path::AbsolutePathResolver`].
//!
//! These tools require absolute paths and perform no directory restriction.
//! Use for unrestricted file system access.
//!
//! # Available Tools
//!
//! - [`ReadTool`] - Read file contents with optional line numbers
//! - [`WriteTool`] - Write content to files
//! - [`EditTool`] - Make exact string replacements
//! - [`GlobTool`] - Find files by glob pattern
//! - [`GrepTool`] - Search file contents by regex

mod edit;
mod glob;
mod grep;
mod read;
mod write;

pub use edit::EditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use read::ReadTool;
pub use write::WriteTool;
