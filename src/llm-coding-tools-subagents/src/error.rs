//! Error types for agent configuration operations.

use crate::parser::AgentParseError;
use std::path::PathBuf;
use thiserror::Error;

/// Error type for agent configuration operations.
#[derive(Debug, Error)]
pub enum AgentLoadError {
    /// File I/O failed.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Frontmatter parsing failed.
    #[error("parse error in {path}: {source}")]
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying parse error.
        #[source]
        source: AgentParseError,
    },

    /// Schema validation failed.
    #[error("schema validation failed in {path}: {message}")]
    SchemaValidation {
        /// Path with invalid schema.
        path: PathBuf,
        /// Validation error message.
        message: String,
    },
}

/// Result type alias for agent configuration operations.
pub type AgentLoadResult<T> = Result<T, AgentLoadError>;
