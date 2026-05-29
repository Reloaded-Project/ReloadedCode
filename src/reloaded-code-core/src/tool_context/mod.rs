//! Tool build context module.
//!
//! Provides `ToolBuildContext` for passing common build-time information
//! when constructing tools. Create one instance before the tool construction
//! loop and pass it to each tool resolver.

use std::path::{Path, PathBuf};

use crate::permissions::Ruleset;
use soft_canonicalize::soft_canonicalize;

/// Context passed when building any tool.
///
/// Create one instance before the tool construction loop and pass it to
/// each tool resolver. The context holds the canonicalized workspace root
/// and optional permission ruleset.
///
/// # Canonicalization
///
/// `workspace_root` is canonicalized once at construction time using
/// `soft_canonicalize`, enabling fail-fast error detection before the
/// tool loop.
#[derive(Debug, Clone)]
pub struct ToolBuildContext<'a> {
    /// Canonicalized root directory of the workspace.
    workspace_root: PathBuf,
    /// Optional permission ruleset for tool access control.
    pub permission: Option<&'a Ruleset>,
}

impl<'a> ToolBuildContext<'a> {
    /// Creates a new `ToolBuildContext` with a canonicalized workspace root.
    ///
    /// # Errors
    /// Returns an error if `workspace_root` cannot be canonicalized.
    pub fn new(
        workspace_root: impl AsRef<Path>,
        permission: Option<&'a Ruleset>,
    ) -> Result<Self, std::io::Error> {
        Ok(Self {
            workspace_root: soft_canonicalize(workspace_root)?,
            permission,
        })
    }

    /// Returns the canonicalized workspace root.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}
