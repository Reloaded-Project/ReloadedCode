//! Allowed directory path resolver implementation.

use super::PathResolver;
use crate::error::{ToolError, ToolResult};
use std::path::PathBuf;

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
/// `glob`, `grep`). If your threat model requires actual filesystem sandboxing, you must
/// either:
///
/// - Disable the bash tool entirely, or
/// - Run the process in an OS-level sandbox (containers, seccomp, landlock, etc.)
#[derive(Debug, Clone)]
pub struct AllowedPathResolver {
    /// Canonicalized allowed base directories.
    allowed_paths: Vec<PathBuf>,
}

impl AllowedPathResolver {
    /// Creates a new resolver with the given allowed directories.
    ///
    /// Each directory is canonicalized during construction to ensure
    /// consistent path comparison. Returns an error if any directory
    /// doesn't exist or can't be canonicalized.
    pub fn new(allowed_paths: Vec<PathBuf>) -> ToolResult<Self> {
        let canonicalized: Result<Vec<_>, _> = allowed_paths
            .into_iter()
            .map(|p| {
                p.canonicalize().map_err(|e| {
                    ToolError::InvalidPath(format!(
                        "failed to canonicalize allowed path '{}': {}",
                        p.display(),
                        e
                    ))
                })
            })
            .collect();

        Ok(Self {
            allowed_paths: canonicalized?,
        })
    }

    /// Creates a resolver from already-canonicalized paths.
    ///
    /// Use this when paths are known to be valid and canonicalized,
    /// skipping the filesystem check.
    ///
    /// # Safety
    ///
    /// Caller must ensure paths are actually canonical. Using non-canonical
    /// paths may allow path traversal attacks.
    pub fn from_canonical(allowed_paths: Vec<PathBuf>) -> Self {
        Self { allowed_paths }
    }

    /// Returns the allowed base directories.
    pub fn allowed_paths(&self) -> &[PathBuf] {
        &self.allowed_paths
    }
}

impl PathResolver for AllowedPathResolver {
    fn resolve(&self, path: &str) -> ToolResult<PathBuf> {
        let input_path = PathBuf::from(path);

        // Try each allowed base directory in order
        for base in &self.allowed_paths {
            let candidate = base.join(&input_path);

            // Try to canonicalize for existing paths
            if let Ok(canonical) = candidate.canonicalize() {
                // Security check: resolved path must stay within allowed base
                if canonical.starts_with(base) {
                    return Ok(canonical);
                }
                // Path escaped allowed directory - try next base
                continue;
            }

            // For non-existent paths (write operations), validate parent
            if let Some(parent) = candidate.parent() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    if canonical_parent.starts_with(base) {
                        // Parent is valid, construct the final path
                        let file_name = candidate.file_name().ok_or_else(|| {
                            ToolError::InvalidPath("path has no file name".into())
                        })?;
                        return Ok(canonical_parent.join(file_name));
                    }
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
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        fs::write(dir.path().join("subdir/nested.txt"), "nested").unwrap();
        dir
    }

    #[test]
    fn resolves_relative_path_in_allowed_dir() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("file.txt");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("file.txt"));
    }

    #[test]
    fn resolves_nested_path() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("subdir/nested.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_path_traversal() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("../../../etc/passwd");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not within allowed"));
    }

    #[test]
    fn allows_non_existent_path_for_write() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("new_file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn allows_nested_non_existent_path() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("subdir/new_file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_non_existent_path_outside_allowed() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        // Parent traversal in non-existent path
        let result = resolver.resolve("subdir/../../../new_file.txt");
        assert!(result.is_err());
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
    fn returns_canonical_path() {
        let dir = setup_test_dir();
        let resolver = AllowedPathResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("subdir/../file.txt");
        assert!(result.is_ok());
        // Should resolve to the canonical path without ../
        let resolved = result.unwrap();
        assert!(!resolved.to_string_lossy().contains(".."));
    }
}
