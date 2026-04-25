//! Path normalization utilities for glob matching.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::error::{ToolError, ToolResult};

/// Normalizes a path to use forward slashes for consistent glob matching.
///
/// On Windows, converts backslashes to forward slashes.
/// On Unix, this returns the path string unchanged.
#[inline]
pub(crate) fn normalize_path(path: &Path) -> Cow<'_, str> {
    let path_str = path.to_string_lossy();
    #[cfg(windows)]
    {
        if path_str.contains('\\') {
            Cow::Owned(path_str.replace('\\', "/"))
        } else {
            path_str
        }
    }
    #[cfg(not(windows))]
    {
        path_str
    }
}

/// Expands shell-like patterns (`~/`, `$HOME/`, `$VAR`, `${VAR:-default}`).
///
/// Returns `Cow::Borrowed` for patterns without shell metacharacters (zero allocation).
/// All other `expand_*` functions in this crate are thin wrappers around this one.
pub(crate) fn expand_pattern(
    pattern: &str,
) -> Result<Cow<'_, str>, shellexpand::LookupError<std::env::VarError>> {
    shellexpand::full(pattern)
}

/// Expands shell-like patterns in a path string, returning a [`PathBuf`].
///
/// Wraps the internal expansion logic with fail-fast error handling: returns
/// `ToolError::InvalidPath` if expansion fails (e.g., unset variable).
///
/// # Errors
/// - Returns [`ToolError::InvalidPath`] when shell expansion fails (e.g., unset
///   environment variable in the path pattern).
pub fn expand_shell(path: &str) -> ToolResult<PathBuf> {
    expand_pattern(path)
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
    use rstest::rstest;
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
    fn expand_shell_produces_pathbuf_on_success() {
        let temp_dir = TempDir::new().unwrap();
        let temp_home = temp_dir.path().canonicalize().unwrap();
        let temp_home = strip_verbatim(temp_home);

        temp_env::with_var("HOME", Some(&temp_home), || {
            let result = strip_verbatim(expand_shell("$HOME/workspace").unwrap());
            assert!(result.starts_with(&temp_home));
            assert!(result.ends_with("workspace"));
        });
    }

    #[test]
    fn expand_shell_returns_error_for_unset_environment_variable() {
        temp_env::with_var("DEFINITELY_NOT_SET_12345", None::<&str>, || {
            let result = expand_shell("$DEFINITELY_NOT_SET_12345/project");
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(err.to_string().contains("failed to expand shell pattern"));
            assert!(err.to_string().contains("DEFINITELY_NOT_SET_12345"));
        });
    }

    // --- expand_pattern ---

    #[cfg(not(windows))]
    #[test]
    fn expand_pattern_should_expand_tilde() {
        let temp_dir = TempDir::new().unwrap();
        let temp_home = temp_dir.path().canonicalize().unwrap();

        temp_env::with_var("HOME", Some(&temp_home), || {
            let expanded = expand_pattern("~/projects/*").unwrap();
            let expanded_str = expanded.as_ref();
            assert!(
                expanded_str.starts_with(temp_home.to_str().unwrap()),
                "expected expansion to start with {:?}, got {:?}",
                temp_home,
                expanded_str
            );
            assert!(
                expanded_str.ends_with("/projects/*"),
                "expected expansion to end with /projects/*, got {:?}",
                expanded_str
            );
        });
    }

    #[test]
    fn expand_pattern_should_expand_dollar_home() {
        let temp_dir = TempDir::new().unwrap();
        let temp_home = temp_dir.path().canonicalize().unwrap();

        temp_env::with_var("HOME", Some(&temp_home), || {
            let expanded = expand_pattern("$HOME/.config/*").unwrap();
            let expanded_str = expanded.as_ref();
            assert!(
                expanded_str.starts_with(temp_home.to_str().unwrap()),
                "expected expansion to start with {:?}, got {:?}",
                temp_home,
                expanded_str
            );
            assert!(
                expanded_str.ends_with("/.config/*"),
                "expected expansion to end with /.config/*, got {:?}",
                expanded_str
            );
        });
    }

    #[rstest]
    #[case::absolute("/workspace/src/lib.rs")]
    #[case::wildcard("*.rs")]
    #[case::prefix_wildcard("orchestrator-*")]
    #[case::exact("bash")]
    #[case::star("*")]
    fn expand_pattern_should_borrow_when_no_shell_chars(#[case] pattern: &str) {
        let result = expand_pattern(pattern).unwrap();
        assert!(
            matches!(result, Cow::Borrowed(_)),
            "expected Borrowed for {:?}, got Owned",
            pattern
        );
        assert_eq!(result.as_ref(), pattern);
    }

    #[test]
    fn expand_pattern_should_return_error_on_failure() {
        temp_env::with_var("DEFINITELY_NOT_SET_99999", None::<&str>, || {
            assert!(expand_pattern("$DEFINITELY_NOT_SET_99999/path").is_err());
        });
    }
}
