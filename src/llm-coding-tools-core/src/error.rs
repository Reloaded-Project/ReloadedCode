//! Common error types for coding tools.

use thiserror::Error;

/// Unified error type for all tool operations.
#[derive(Debug, Error)]
pub enum ToolError {
    /// File I/O operation failed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Path validation failed (not absolute, doesn't exist, etc.).
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// Requested offset/limit exceeds file bounds.
    #[error("out of bounds: {0}")]
    OutOfBounds(String),

    /// Glob/regex pattern is invalid.
    #[error("invalid pattern: {0}")]
    InvalidPattern(String),

    /// HTTP request failed.
    #[error("HTTP error: {0}")]
    Http(String),

    /// Command execution failed.
    #[error("execution error: {0}")]
    Execution(String),

    /// Timeout exceeded.
    #[error("timeout: {0}")]
    Timeout(String),

    /// Validation failed.
    #[error("validation error: {0}")]
    Validation(String),

    /// JSON serialization/deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type alias for tool operations.
pub type ToolResult<T> = Result<T, ToolError>;

impl From<globset::Error> for ToolError {
    fn from(e: globset::Error) -> Self {
        ToolError::InvalidPattern(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_error_displays_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: ToolError = io_err.into();
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn tool_error_displays_invalid_path() {
        let err = ToolError::InvalidPath("not absolute".into());
        assert!(err.to_string().contains("invalid path"));
    }

    #[test]
    fn tool_error_from_glob_pattern_error() {
        let glob_err = globset::Glob::new("[invalid").unwrap_err();
        let err: ToolError = glob_err.into();
        assert!(matches!(err, ToolError::InvalidPattern(_)));
    }
}
