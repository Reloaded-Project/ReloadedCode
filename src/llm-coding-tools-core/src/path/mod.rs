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

/// Resolves a path for a new file when the parent directory exists.
///
/// This is a fast path optimization that avoids the expensive `soft_canonicalize`
/// for the common case where a new file is being created in an existing directory.
///
/// # Platform Differences
///
/// - **Unix**: Canonicalizes the parent directory and joins the filename.
///   This is safe because Unix path resolution is straightforward.
/// - **Windows/others**: Uses `soft_canonicalize` because Windows has complex path
///   semantics (drive letters, UNC paths, verbatim paths) that require the
///   full resolution logic for correct `..` handling.
///
/// # Returns
///
/// - `Some(resolved_path)` if the parent directory exists and was successfully canonicalized
/// - `None` if the parent directory doesn't exist or canonicalization failed
#[inline]
pub(crate) fn resolve_new_file_fast(candidate: &Path) -> Option<PathBuf> {
    let parent = candidate.parent()?;

    #[cfg(unix)]
    {
        let filename = candidate.file_name()?;
        if parent.is_dir() {
            return parent.canonicalize().ok().map(|p| p.join(filename));
        }
        None
    }

    #[cfg(not(unix))]
    {
        if parent.is_dir() {
            return soft_canonicalize::soft_canonicalize(candidate).ok();
        }
        None
    }
}
