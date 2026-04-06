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

/// Resolves a path that does not exist yet by walking up to the first parent
/// directory that does exist.
///
/// Starts from `candidate.parent()` so we do not retry the failed
/// `candidate.canonicalize()` call from the caller. After we find an existing
/// parent, we add the missing path back onto it in a way that still blocks
/// `..` from escaping above that parent.
pub(super) fn resolve_nonexistent_candidate(base: &Path, candidate: &Path) -> Option<PathBuf> {
    let mut ancestor = candidate.parent()?;

    loop {
        match ancestor.canonicalize() {
            Ok(resolved_ancestor) => {
                if !resolved_ancestor.starts_with(base) {
                    return None;
                }

                let remaining = candidate.strip_prefix(ancestor).ok()?;
                return join_remaining_suffix(&resolved_ancestor, remaining);
            }
            Err(_) => {
                ancestor = ancestor.parent()?;
            }
        }
    }
}

/// Adds the missing part of the path back onto the resolved parent.
///
/// We do not use `resolved_ancestor.join(remaining)` directly because it keeps
/// `.` and `..` as-is. For a path that does not exist yet, there is no final
/// filesystem check to catch that, so we clean up the suffix here and reject
/// any `..` that would move above `resolved_ancestor`.
fn join_remaining_suffix(resolved_ancestor: &Path, remaining: &Path) -> Option<PathBuf> {
    let mut target = resolved_ancestor.to_path_buf();
    let mut appended_depth = 0usize;

    for component in remaining.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => {
                target.push(segment);
                appended_depth += 1;
            }
            Component::ParentDir => {
                if appended_depth == 0 {
                    return None;
                }
                let popped = target.pop();
                debug_assert!(popped, "target should contain appended components");
                appended_depth -= 1;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(target)
}
