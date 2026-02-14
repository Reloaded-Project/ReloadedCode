//! Shell command execution operation.

use crate::ToolOutput;
use core::fmt::Write;
use serde::Serialize;

/// Default buffer capacity for stdout/stderr pipe reads.
/// 32KB covers typical command output without reallocations.
const PIPE_BUFFER_CAPACITY: usize = 32 * 1024;

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

impl BashOutput {
    /// Formats the bash output into a [`ToolOutput`] for LLM consumption.
    ///
    /// Combines stdout, stderr (with `[stderr]` label), and non-zero exit codes
    /// into a single formatted string.
    pub fn format_output(&self) -> ToolOutput {
        // Pre-allocate: stdout + stderr + labels overhead (~34 bytes)
        // 34 bytes assumes the exit code is up to 10 digits, i.e. int32 range.
        let estimated = self.stdout.len() + self.stderr.len() + 34;
        let mut content = String::with_capacity(estimated);

        if !self.stdout.is_empty() {
            content.push_str(&self.stdout);
        }

        if !self.stderr.is_empty() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str("[stderr]\n");
            content.push_str(&self.stderr);
        }

        if let Some(code) = self.exit_code {
            if code != 0 {
                if !content.is_empty() {
                    content.push('\n');
                }
                // Use write! to avoid format! allocation
                let _ = write!(content, "[exit code: {code}]");
            }
        }

        ToolOutput::new(content)
    }
}

#[cfg(feature = "tokio")]
mod tokio_impl;
#[cfg(feature = "tokio")]
pub use tokio_impl::execute_command;

#[cfg(all(feature = "blocking", not(feature = "tokio")))]
mod blocking_impl;
#[cfg(all(feature = "blocking", not(feature = "tokio")))]
pub use blocking_impl::execute_command;
