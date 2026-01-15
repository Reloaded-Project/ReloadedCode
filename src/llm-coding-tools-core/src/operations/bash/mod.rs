//! Shell command execution operation.

use serde::Serialize;

/// Result of shell command execution.
#[derive(Debug, Clone, Serialize)]
pub struct BashOutput {
    /// Exit code from the command (None if killed by timeout).
    pub exit_code: Option<i32>,
    /// Standard output from the command.
    pub stdout: String,
    /// Standard error output from the command.
    pub stderr: String,
}

#[cfg(not(feature = "blocking"))]
mod async_impl;
#[cfg(not(feature = "blocking"))]
pub use async_impl::execute_command;

#[cfg(feature = "blocking")]
mod blocking_impl;
#[cfg(feature = "blocking")]
pub use blocking_impl::execute_command;
