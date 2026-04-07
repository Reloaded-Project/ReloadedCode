//! Path resolution strategies for tool security.
//!
//! This module provides [`PathResolver`] trait and implementations:
//! - [`AbsolutePathResolver`] - Requires absolute paths only
//! - [`AllowedPathResolver`] - Restricts to allowed directories
//! - [`AllowedGlobResolver`] - Restricts to allowed directories with glob pattern filtering

mod absolute;
mod allowed;
mod allowed_glob;

pub use absolute::AbsolutePathResolver;
pub use allowed::AllowedPathResolver;
pub use allowed_glob::{AllowedGlobResolver, GlobPolicy, GlobPolicyBuilder, RuleAction};

use crate::context::PathMode;
use crate::error::ToolResult;
use std::path::{Component, Path, PathBuf};

/// Strategy for resolving and validating file paths.
///
/// Implementations control whether paths must be absolute, relative to
/// allowed directories, or follow other constraints.
pub trait PathResolver: Send + Sync {
    /// Describes how tools should present paths for this resolver.
    ///
    /// Custom resolvers default to [`PathMode::Absolute`] unless they opt into
    /// [`PathMode::Allowed`].
    const PATH_MODE: PathMode = PathMode::Absolute;

    /// Resolves and validates a path string.
    ///
    /// Returns an absolute path (may or may not be canonical) if valid,
    /// or an error describing the issue.
    fn resolve(&self, path: &str) -> ToolResult<PathBuf>;
}

/// Fast lexical check for whether a relative path would escape its base directory.
///
/// This is a cheap pre-filter that avoids filesystem operations for obvious traversal
/// attacks. It tracks the effective depth while walking path components:
/// - `.` (current directory) has no effect
/// - normal components increase depth
/// - `..` (parent directory) decreases depth, and if depth is already 0, the path escapes
///
/// # Returns
///
/// - `true` if the path would escape (e.g., `../../../etc/passwd`, `../secrets.txt`)
/// - `false` if the path stays within bounds or is absolute
#[inline]
pub(crate) fn relative_path_escapes_base(path: &Path) -> bool {
    if path.is_absolute() {
        return false;
    }

    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => {
                if depth == 0 {
                    return true;
                }
                depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }

    false
}
