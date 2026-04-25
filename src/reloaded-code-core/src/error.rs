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

    /// Timeout with kill failure - process may still be running.
    #[error("timeout: {message}\n(kill failed: {kill_error})")]
    TimeoutWithKillFailure {
        /// Timeout message including context.
        message: String,
        /// Kill error message.
        kill_error: String,
    },

    /// Validation failed.
    #[error("validation error: {message}")]
    Validation {
        /// Field that failed validation, if applicable.
        field: Option<String>,
        /// Validation error message.
        message: String,
    },

    /// JSON serialization/deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Permission denied for the requested operation.
    #[error("permission denied for tool '{tool}' on '{subject}'")]
    PermissionDenied {
        /// Tool name that was denied.
        tool: &'static str,
        /// Path or command that was denied.
        subject: String,
    },
}

/// Result type alias for tool operations.
pub type ToolResult<T> = Result<T, ToolError>;

impl From<globset::Error> for ToolError {
    fn from(e: globset::Error) -> Self {
        ToolError::InvalidPattern(e.to_string())
    }
}

impl ToolError {
    /// Create a validation error without a specific field.
    #[must_use]
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            field: None,
            message: message.into(),
        }
    }

    /// Create a validation error for a specific field.
    #[must_use]
    pub fn validation_for(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: Some(field.into()),
            message: message.into(),
        }
    }
}
