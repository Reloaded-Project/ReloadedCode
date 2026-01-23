//! Error types for agent configuration operations.

use std::path::PathBuf;
use thiserror::Error;

/// Error type for agent configuration operations.
#[derive(Debug, Error)]
pub enum AgentConfigError {
    /// File I/O failed.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// No frontmatter delimiters found in file.
    #[error("missing frontmatter in {path}")]
    MissingFrontmatter {
        /// Path missing frontmatter.
        path: PathBuf,
    },

    /// YAML parsing failed.
    #[error("invalid YAML frontmatter in {path}: {message}")]
    InvalidYaml {
        /// Path with invalid YAML.
        path: PathBuf,
        /// YAML parser error message.
        message: String,
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
pub type AgentConfigResult<T> = Result<T, AgentConfigError>;
