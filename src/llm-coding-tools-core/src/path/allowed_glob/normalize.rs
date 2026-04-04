//! Path normalization utilities for glob matching.

use std::path::{Path, PathBuf};

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
/// Returns the expanded path or the original if no expansion needed. Uses
/// `shellexpand` which internally uses `dirs::home_dir()` for cross-platform
/// home detection.
pub(crate) fn expand_shell(path: &str) -> PathBuf {
    PathBuf::from(
        shellexpand::full(path)
            .unwrap_or_else(|_| path.into())
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        use temp_env::with_var;
        use tempfile::TempDir;

        let temp_home_path = TempDir::new().unwrap().path().canonicalize().unwrap();

        #[cfg(windows)]
        let env_var = "USERPROFILE";
        #[cfg(not(windows))]
        let env_var = "HOME";

        with_var(env_var, Some(&temp_home_path), || {
            let result = strip_verbatim(expand_shell("~/project"));
            assert!(result.starts_with(&temp_home_path));
            assert!(result.ends_with("project"));
        });
    }

    #[test]
    fn expands_home_dollar() {
        use temp_env::with_var;
        use tempfile::TempDir;

        let temp_home_path = TempDir::new().unwrap().path().canonicalize().unwrap();

        with_var("HOME", Some(&temp_home_path), || {
            let result = strip_verbatim(expand_shell("$HOME/workspace"));
            assert!(result.starts_with(&temp_home_path));
            assert!(result.ends_with("workspace"));
        });
    }

    #[test]
    fn leaves_path_without_shell_patterns_unchanged() {
        let result = expand_shell("/some/path");
        assert_eq!(result, PathBuf::from("/some/path"));
    }
}
