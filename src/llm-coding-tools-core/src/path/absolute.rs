//! Absolute path resolver implementation.

use super::PathResolver;
use crate::error::{ToolError, ToolResult};
use std::path::PathBuf;

/// Path resolver that requires absolute paths.
///
/// This is the simplest resolver - it validates that paths are absolute
/// and returns them as-is. No directory restrictions are applied.
///
/// # Example
///
/// ```
/// use llm_coding_tools_core::path::{PathResolver, AbsolutePathResolver};
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

    #[test]
    fn accepts_absolute_path() {
        let resolver = AbsolutePathResolver;
        #[cfg(windows)]
        let path = "C:\\Users\\user\\file.txt";
        #[cfg(not(windows))]
        let path = "/home/user/file.txt";
        
        let result = resolver.resolve(path);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from(path));
    }

    #[test]
    fn rejects_relative_path() {
        let resolver = AbsolutePathResolver;
        let result = resolver.resolve("relative/path.txt");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::InvalidPath(_)));
        assert!(err.to_string().contains("must be absolute"));
    }

    #[test]
    fn rejects_dot_relative_path() {
        let resolver = AbsolutePathResolver;
        assert!(resolver.resolve("./file.txt").is_err());
        assert!(resolver.resolve("../file.txt").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn accepts_windows_absolute_path() {
        let resolver = AbsolutePathResolver;
        assert!(resolver.resolve("C:\\Users\\file.txt").is_ok());
    }
}
