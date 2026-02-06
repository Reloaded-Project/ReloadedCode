//! Error types for agent configuration operations.

use crate::parser::AgentParseError;
use std::fmt;
use std::path::PathBuf;

/// Error type for agent configuration operations.
#[derive(Debug)]
pub enum AgentLoadError {
    /// File I/O failed.
    Io {
        /// Path that failed to read, or None for in-memory sources.
        path: Option<PathBuf>,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Frontmatter parsing failed.
    Parse {
        /// Path that failed to parse, or None for in-memory sources.
        path: Option<PathBuf>,
        /// Underlying parse error.
        source: AgentParseError,
    },

    /// Schema validation failed.
    SchemaValidation {
        /// Path with invalid schema, or None for in-memory sources.
        path: Option<PathBuf>,
        /// Validation error message.
        message: String,
    },
}

impl fmt::Display for AgentLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentLoadError::Io { path, source } => {
                let path_str = path
                    .as_deref()
                    .map_or("<memory>", |p| p.to_str().unwrap_or("<invalid>"));
                write!(f, "I/O error reading {path_str}: {source}")
            }
            AgentLoadError::Parse { path, source } => {
                let path_str = path
                    .as_deref()
                    .map_or("<memory>", |p| p.to_str().unwrap_or("<invalid>"));
                write!(f, "parse error in {path_str}: {source}")
            }
            AgentLoadError::SchemaValidation { path, message } => {
                let path_str = path
                    .as_deref()
                    .map_or("<memory>", |p| p.to_str().unwrap_or("<invalid>"));
                write!(f, "schema validation failed in {path_str}: {message}")
            }
        }
    }
}

impl std::error::Error for AgentLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AgentLoadError::Io { source, .. } => Some(source),
            AgentLoadError::Parse { source, .. } => Some(source),
            AgentLoadError::SchemaValidation { .. } => None,
        }
    }
}

impl AgentLoadError {
    /// Creates a new Io error.
    pub fn io(path: Option<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path, source }
    }

    /// Creates a new Parse error.
    pub fn parse(path: Option<PathBuf>, source: AgentParseError) -> Self {
        Self::Parse { path, source }
    }

    /// Creates a new SchemaValidation error.
    pub fn schema_validation(path: Option<PathBuf>, message: impl Into<String>) -> Self {
        Self::SchemaValidation {
            path,
            message: message.into(),
        }
    }
}

/// Result type alias for agent configuration operations.
pub type AgentLoadResult<T> = Result<T, AgentLoadError>;
