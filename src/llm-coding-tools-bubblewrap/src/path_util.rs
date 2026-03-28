//! Crate-private path utilities for preparing filesystem paths before sandbox use.
//!
//! Callers in [`probe`](crate::probe) and [`profile`](crate::profile) use these
//! helpers to normalize paths so that comparisons and bind-mount targets are
//! consistent regardless of symlinks or relative components.

use std::fs;
use std::path::Path;

/// Canonicalizes `path` when possible and otherwise preserves the original.
pub(crate) fn normalize_path(path: &Path) -> Box<Path> {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .into_boxed_path()
}
