//! Absolute path resolver implementation.

use super::PathResolver;
use crate::context::PathMode;
use crate::error::{ToolError, ToolResult};
use std::path::{Path, PathBuf};

/// Path resolver that requires absolute paths.
///
/// This is the simplest resolver - it validates that paths are absolute
/// and returns them as-is. No directory restrictions are applied.
///
/// # Example
///
/// ```
/// use reloaded_code_core::path::{PathResolver, AbsolutePathResolver};
///
/// let resolver = AbsolutePathResolver;
/// #[cfg(windows)]
/// assert!(resolver.resolve("C:\\Users\\user\\file.txt").is_ok());
/// #[cfg(not(windows))]
/// assert!(resolver.resolve("/home/user/file.txt").is_ok());
/// assert!(resolver.resolve("relative/path.txt").is_err());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct AbsolutePathResolver;

impl PathResolver for AbsolutePathResolver {
    fn path_mode(&self) -> PathMode {
        PathMode::Absolute
    }

    fn is_path_allowed(&self, _path: &Path) -> bool {
        true
    }

    /// # Errors
    /// - Returns [`ToolError::InvalidPath`] when `path` is not an absolute path.
    fn resolve(&self, path: &str) -> ToolResult<PathBuf> {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(ToolError::InvalidPath(format!(
                "path must be absolute: {}",
                path.display()
            )));
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[cfg_attr(
        windows,
        case::accepts_platform_absolute_path(
            "C:\\Users\\user\\file.txt",    // input: Windows absolute path
            Ok("C:\\Users\\user\\file.txt") // expected: accepted as-is
        )
    )]
    #[cfg_attr(
        not(windows),
        case::accepts_platform_absolute_path(
            "/home/user/file.txt",    // input: Unix absolute path
            Ok("/home/user/file.txt") // expected: accepted as-is
        )
    )]
    #[case::rejects_plain_relative_path(
        "relative/path.txt", // input: plain relative, no dot prefix
        Err(())              // expected: rejected as non-absolute
    )]
    #[case::rejects_dot_relative_path(
        "./file.txt", // input: dot-relative path
        Err(())       // expected: rejected as non-absolute
    )]
    #[case::rejects_parent_relative_path(
        "../file.txt", // input: parent-relative path
        Err(())        // expected: rejected as non-absolute
    )]
    #[cfg_attr(
        windows,
        case::accepts_windows_absolute_path(
            "C:\\Users\\file.txt",    // input: Windows absolute with different path
            Ok("C:\\Users\\file.txt") // expected: accepted as-is
        )
    )]
    fn resolve_handles_absolute_and_relative_paths(
        #[case] input: &str,                        // path string to resolve
        #[case] expected: Result<&'static str, ()>, // Ok(path) if accepted, Err(()) if rejected
    ) {
        let resolver = AbsolutePathResolver;
        let result = resolver.resolve(input);
        match expected {
            Ok(expected_path) => assert_eq!(result.unwrap(), PathBuf::from(expected_path)),
            Err(()) => {
                let err = result.unwrap_err();
                assert!(matches!(err, ToolError::InvalidPath(_)));
                assert!(err.to_string().contains("must be absolute"));
            }
        }
    }

    #[test]
    fn is_path_allowed_always_true() {
        let resolver = AbsolutePathResolver;
        // resolve succeeds for absolute paths
        let ok_path = if cfg!(windows) {
            "C:\\Users\\file.txt"
        } else {
            "/home/user/file.txt"
        };
        let resolved = resolver.resolve(ok_path).unwrap();
        assert!(resolver.is_path_allowed(&resolved));
        // is_path_allowed returns true even for paths that resolve() would reject
        assert!(resolver.is_path_allowed(Path::new("relative/path")));
    }
}
