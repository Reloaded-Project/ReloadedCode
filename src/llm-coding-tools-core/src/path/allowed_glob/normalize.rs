//! Path normalization utilities for glob matching.

use std::path::{Path, PathBuf};

use crate::error::{ToolError, ToolResult};

/// Normalizes a path to use forward slashes for consistent glob matching.
///
/// On Windows, converts backslashes to forward slashes.
/// On Unix, this returns the path string unchanged.
pub(crate) fn normalize_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    #[cfg(windows)]
    {
        path_str.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path_str.into_owned()
    }
}

/// Expands shell-like patterns (`~/`, `$HOME/`, `$VAR`, `${VAR:-default}`) in a
/// path string.
///
/// Returns the expanded path on success, or a `ToolError::InvalidPath` if
/// expansion fails (e.g., environment variable not set or contains non-Unicode
/// data). Uses `shellexpand` which internally uses `dirs::home_dir()` for
/// cross-platform home detection.
pub(crate) fn expand_shell(path: &str) -> ToolResult<PathBuf> {
    shellexpand::full(path)
        .map(|cow| PathBuf::from(cow.into_owned()))
        .map_err(|e| {
            ToolError::InvalidPath(format!(
                "failed to expand shell pattern in path '{}': {}",
                path, e
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use temp_env;
    use tempfile::TempDir;

    #[cfg(windows)]
    fn strip_verbatim(path: PathBuf) -> PathBuf {
        PathBuf::from(
            path.to_string_lossy()
                .strip_prefix(r"\\?\")
                .unwrap_or(&path.to_string_lossy()),
        )
    }

    #[cfg(not(windows))]
    fn strip_verbatim(path: PathBuf) -> PathBuf {
        path
    }

    #[test]
    fn normalize_path_converts_backslashes_on_windows() {
        #[cfg(windows)]
        {
            assert_eq!(normalize_path(Path::new("src\\lib.rs")), "src/lib.rs");
            assert_eq!(
                normalize_path(Path::new("src\\deep\\nested\\mod.rs")),
                "src/deep/nested/mod.rs"
            );
            assert_eq!(
                normalize_path(Path::new("C:\\Users\\test\\project")),
                "C:/Users/test/project"
            );
            assert_eq!(
                normalize_path(Path::new("src/lib\\mod.rs")),
                "src/lib/mod.rs"
            );
        }

        #[cfg(not(windows))]
        {
            assert_eq!(normalize_path(Path::new("src/lib.rs")), "src/lib.rs");
            assert_eq!(
                normalize_path(Path::new("src/deep/nested/mod.rs")),
                "src/deep/nested/mod.rs"
            );
        }
    }

    #[test]
    fn expands_home_tilde() {
        #[cfg(windows)]
        {
            let expected_home = dirs::home_dir().expect("home directory should exist");
            let expected_home = strip_verbatim(expected_home.canonicalize().unwrap());

            let result = strip_verbatim(expand_shell("~/project").unwrap());
            assert!(result.starts_with(&expected_home));
            assert!(result.ends_with("project"));
        }

        #[cfg(not(windows))]
        {
            let temp_dir = TempDir::new().unwrap();
            let temp_home_path = temp_dir.path().canonicalize().unwrap();
            temp_env::with_var("HOME", Some(&temp_home_path), || {
                let result = expand_shell("~/project").unwrap();
                assert!(result.starts_with(&temp_home_path));
                assert!(result.ends_with("project"));
            });
        }
    }

    #[test]
    fn expands_home_dollar() {
        let temp_dir = TempDir::new().unwrap();
        let temp_home_path = temp_dir.path().canonicalize().unwrap();
        let temp_home_path = strip_verbatim(temp_home_path);

        temp_env::with_var("HOME", Some(&temp_home_path), || {
            let result = strip_verbatim(expand_shell("$HOME/workspace").unwrap());
            assert!(result.starts_with(&temp_home_path));
            assert!(result.ends_with("workspace"));
        });
    }

    #[test]
    fn leaves_path_without_shell_patterns_unchanged() {
        let result = expand_shell("/some/path").unwrap();
        assert_eq!(result, PathBuf::from("/some/path"));
    }

    #[test]
    fn returns_error_for_unset_environment_variable() {
        temp_env::with_var("DEFINITELY_NOT_SET_12345", None::<&str>, || {
            let result = expand_shell("$DEFINITELY_NOT_SET_12345/project");
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(err.to_string().contains("failed to expand shell pattern"));
            assert!(err.to_string().contains("DEFINITELY_NOT_SET_12345"));
        });
    }
}
