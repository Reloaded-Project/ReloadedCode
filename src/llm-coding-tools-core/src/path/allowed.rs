//! Allowed directory path resolver implementation.

use super::{relative_path_escapes_base, resolve_new_file_fast, PathResolver};
use crate::context::PathMode;
use crate::error::{ToolError, ToolResult};
use soft_canonicalize::soft_canonicalize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Path resolver that restricts access to allowed directories.
///
/// Paths are resolved relative to configured base directories.
/// Prevents path traversal attacks by validating resolved paths
/// stay within allowed boundaries.
///
/// # Security
///
/// This resolver protects against path traversal by:
/// 1. Canonicalizing the resolved path to eliminate `..` and symlinks
/// 2. Verifying the result starts with an allowed base directory
///
/// ## Bash Tool Bypasses Path Restrictions
///
/// **When the bash/shell tool is enabled, this resolver's protections are effectively
/// advisory.** The bash tool permits arbitrary shell commands, meaning an LLM can
/// directly read, write, or delete any file the process has OS-level permissions for
/// (e.g., `cat /etc/passwd`, `rm -rf /`, `curl ... | sh`).
///
/// This resolver only restricts the structured file operations (`read`, `write`, `edit`,
/// `glob`, `grep`). It does not make shell execution safe.
/// See `SANDBOX-PROFILES.md` for details on sandboxing on Linux.
#[derive(Debug, Clone)]
pub struct AllowedPathResolver {
    /// Canonicalized allowed base directories.
    allowed_paths: Arc<[PathBuf]>,
}

impl AllowedPathResolver {
    /// Creates a new resolver with the given allowed directories.
    ///
    /// Each directory is resolved during construction to ensure consistent path
    /// comparison. Returns an error if any directory doesn't exist or can't be
    /// resolved.
    pub fn new(allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> ToolResult<Self> {
        let canonicalized: Result<Arc<[PathBuf]>, _> = allowed_paths
            .into_iter()
            .map(|p| {
                let path = p.as_ref();
                if !path.is_dir() {
                    return Err(ToolError::InvalidPath(format!(
                        "failed to resolve allowed path '{}': path is not an existing directory",
                        path.display()
                    )));
                }

                soft_canonicalize(path).map_err(|e| {
                    ToolError::InvalidPath(format!(
                        "failed to resolve allowed path '{}': {}",
                        path.display(),
                        e
                    ))
                })
            })
            .collect();

        Ok(Self {
            allowed_paths: canonicalized?,
        })
    }

    /// Creates a resolver from already-canonicalized paths, skipping
    /// filesystem validation.
    ///
    /// A canonical path is absolute, with all symlinks resolved and all
    /// `.` and `..` components normalized. Use [`std::fs::canonicalize`] or
    /// [`std::path::Path::canonicalize`] to canonicalize paths.
    ///
    /// Use this when paths are known to be valid and canonicalized, skipping
    /// the filesystem check.
    ///
    /// # Safety
    ///
    /// Caller must ensure paths are actually canonical. Using non-canonical
    /// paths may allow path traversal attacks.
    pub fn from_canonical(allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> Self {
        Self {
            allowed_paths: allowed_paths
                .into_iter()
                .map(|p| p.as_ref().to_path_buf())
                .collect(),
        }
    }

    /// Returns the allowed base directories.
    pub fn allowed_paths(&self) -> &[PathBuf] {
        &self.allowed_paths
    }
}

impl PathResolver for AllowedPathResolver {
    const PATH_MODE: PathMode = PathMode::Allowed;

    fn resolve(&self, path: &str) -> ToolResult<PathBuf> {
        let input_path = Path::new(path);

        if relative_path_escapes_base(input_path) {
            return Err(ToolError::InvalidPath(format!(
                "path '{}' is not within allowed directories",
                path
            )));
        }

        // Try each allowed base directory in order
        for base in self.allowed_paths.iter() {
            let candidate = base.join(input_path);

            // Try to canonicalize for existing paths
            if let Ok(canonical) = candidate.canonicalize() {
                // Security check: resolved path must stay within allowed base
                if canonical.starts_with(base) {
                    return Ok(canonical);
                }
                // Path escaped allowed directory - try next base
                continue;
            }

            // Fast path for new files in existing directories.
            // Canonicalizes parent directory, then joins filename.
            // This avoids soft_canonicalize's expensive walk-up logic.
            if let Some(resolved) = resolve_new_file_fast(&candidate) {
                if resolved.starts_with(base) {
                    return Ok(resolved);
                }
                continue;
            }

            // Fallback for paths where parent doesn't exist.
            if let Ok(resolved) = soft_canonicalize(&candidate) {
                if resolved.starts_with(base) {
                    return Ok(resolved);
                }
            }
        }

        Err(ToolError::InvalidPath(format!(
            "path '{}' is not within allowed directories",
            path
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        fs::write(dir.path().join("subdir/nested.txt"), "nested").unwrap();
        dir
    }

    /// Verifies that valid paths resolve successfully, including both existing
    /// files and new files that don't exist yet (important for write operations).
    #[rstest]
    #[case::existing_file_in_root("file.txt", "file.txt")] // exists: created by setup_test_dir()
    #[case::nested_existing_file("subdir/nested.txt", "nested.txt")] // exists: created by setup_test_dir()
    #[case::new_file_in_root("new_file.txt", "new_file.txt")] // does NOT exist: tests write path resolution
    #[case::new_file_in_subdir("subdir/new_file.txt", "new_file.txt")] // does NOT exist: tests write path resolution
    #[case::new_file_in_missing_directories("new_dir/nested/new_file.txt", "new_file.txt")]
    fn resolves_valid_paths_successfully(
        #[case] input_path: &str,
        #[case] expected_filename: &str,
    ) {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve(input_path);
        let resolved = result.expect("path should resolve successfully");
        assert!(
            resolved.ends_with(expected_filename),
            "resolved path should end with '{expected_filename}'"
        );
    }

    /// Verifies that path traversal attempts are blocked regardless of
    /// how the escape is constructed.
    #[rstest]
    #[case::parent_traversal("../../../etc/passwd")]
    #[case::nested_parent_traversal("subdir/../../../new_file.txt")]
    #[case::missing_dir_parent_traversal("new_dir/../../new_file.txt")]
    fn rejects_paths_that_escape_allowed_directory(#[case] input_path: &str) {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve(input_path);
        let err = result.expect_err("path should be rejected");
        assert!(
            err.to_string().contains("not within allowed"),
            "error should mention 'not within allowed'"
        );
    }

    #[test]
    fn resolves_existing_file_through_missing_directory_parent_traversal() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("subdir/new_dir/../../file.txt");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("file.txt"));
    }

    #[test]
    fn tries_multiple_allowed_paths() {
        let dir1 = setup_test_dir();
        let dir2 = setup_test_dir();
        fs::write(dir2.path().join("only_in_dir2.txt"), "content").unwrap();

        let resolver =
            AllowedPathResolver::new(vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()])
                .unwrap();

        // File only exists in dir2
        let result = resolver.resolve("only_in_dir2.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn returns_canonical_path_without_dotdots() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        // Path with ".." should be normalized
        let resolved = resolver.resolve("subdir/../file.txt").unwrap();
        assert!(
            !resolved.to_string_lossy().contains(".."),
            "canonical path should not contain '..'"
        );
    }
}
