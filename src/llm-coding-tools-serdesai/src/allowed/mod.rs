//! Tools using [`llm_coding_tools_core::path::AllowedPathResolver`].
//!
//! These tools restrict file access to configured allowed directories.
//! Use for sandboxed file system access.
//!
//! # Available Tools
//!
//! - [`ReadTool`] - Read file contents within allowed paths
//! - [`WriteTool`] - Write file contents within allowed paths
//! - [`EditTool`] - Edit file with search/replace within allowed paths
//! - [`GlobTool`] - Find files by pattern within allowed paths
//! - [`GrepTool`] - Search file contents within allowed paths

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
