//! Tools using [`llm_coding_tools_core::path::AllowedPathResolver`].
//!
//! These tools restrict file access to configured allowed directories.
//! Use for sandboxed file system access.
//! # Available Tools
//!
//! - [`ReadTool`] - Read file contents within allowed paths
//! - [`WriteTool`] - Write file contents within allowed paths
//! - [`EditTool`] - Edit file with search/replace within allowed paths
//! - [`GlobTool`] - Find files by pattern within allowed paths
//! - [`GrepTool`] - Search file contents within allowed paths
//!
//! [`AllowedPathResolver`]: llm_coding_tools_core::path::AllowedPathResolver

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
