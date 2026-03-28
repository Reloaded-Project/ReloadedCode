//! Error types for bubblewrap sandbox setup and execution.

use thiserror::Error;

/// Errors returned while validating or planning a bubblewrap command line.
#[derive(Debug, Error)]
pub enum LinuxBwrapError {
    /// A caller-provided path (working directory, mount source or destination,
    /// or tmp backing directory) is invalid or unreachable inside the sandbox.
    #[error("{0}")]
    InvalidPath(String),
    /// The `bwrap` binary could not be found on `PATH` or no usable host shell
    /// is visible inside the sandbox.
    #[error("{0}")]
    Execution(String),
}
