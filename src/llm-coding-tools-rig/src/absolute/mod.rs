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

pub use edit::{EditArgs, EditError, EditTool};
pub use glob::{GlobArgs, GlobTool};
pub use grep::{GrepArgs, GrepTool};
pub use read::{ReadArgs, ReadTool};
pub use write::{WriteTool, WriteToolArgs};
