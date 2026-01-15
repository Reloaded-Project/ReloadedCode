//! Path resolution strategies for tool security.
//!
//! This module provides [`PathResolver`] trait and implementations:
//! - [`AbsolutePathResolver`] - Requires absolute paths only
//! - [`AllowedPathResolver`] - Restricts to allowed directories

mod absolute;
mod allowed;

pub use absolute::AbsolutePathResolver;
pub use allowed::AllowedPathResolver;

use crate::error::ToolResult;
use std::path::PathBuf;

/// Strategy for resolving and validating file paths.
///
/// Implementations control whether paths must be absolute, relative to
/// allowed directories, or follow other constraints.
pub trait PathResolver: Send + Sync {
    /// Resolves and validates a path string.
    ///
    /// Returns an absolute path (may or may not be canonical) if valid,
    /// or an error describing the issue.
    fn resolve(&self, path: &str) -> ToolResult<PathBuf>;
}
