//! Run shell commands on the host or inside a Linux bubblewrap sandbox.
//!
//! # Public API
//! - [`execute_command`] / [`execute_command_with_mode`] - Run a shell command with host or sandbox mode.
//! - [`BashExecutionMode`] - Select `Host` or `LinuxBwrap` execution.
//! - [`BashOutput`] - Captured stdout, stderr, and exit code.
//!
//! # Linux Sandbox
#![cfg_attr(
    all(feature = "linux-bubblewrap", target_os = "linux"),
    doc = "Enable the `linux-bubblewrap` feature on Linux to wrap commands in a bubblewrap sandbox."
)]
#![cfg_attr(
    all(feature = "linux-bubblewrap", target_os = "linux"),
    doc = "Build a profile with `linux_bwrap_profile::Builder`:"
)]
#![cfg_attr(
    all(feature = "linux-bubblewrap", target_os = "linux"),
    doc = "- `Builder::public_bot` for untrusted input (no network, filtered mounts, cleared env)."
)]
#![cfg_attr(
    all(feature = "linux-bubblewrap", target_os = "linux"),
    doc = "- `Builder::trusted_maintenance` for trusted jobs (network enabled, read-only host rootfs)."
)]
//!
//! See <https://github.com/Reloaded-Project/ReloadedCode/blob/main/SANDBOX-PROFILES.md>
//! for the full operator guide.
//!
//! # Errors
//! - [`ToolError::PermissionDenied`] when the command is blocked by [`BashSettings::permission`].
//! - [`ToolError::InvalidPath`] when the working directory is not absolute or does not exist.
//! - [`ToolError::Execution`] when the process cannot start, or when `bwrap` is missing or unusable in sandbox mode.
//! - [`ToolError::Timeout`] / [`ToolError::TimeoutWithKillFailure`] when the command exceeds the deadline.

use crate::error::{ToolError, ToolResult};
use crate::permissions::Ruleset;
use crate::ToolOutput;
use core::fmt::Write;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::path::Path;
use std::time::Duration;
#[cfg(feature = "tokio")]
mod tokio_impl;
#[cfg(feature = "tokio")]
pub use tokio_impl::{execute_command, execute_command_with_mode};

#[cfg(all(feature = "blocking", not(feature = "tokio")))]
mod blocking_impl;
#[cfg(all(feature = "blocking", not(feature = "tokio")))]
pub use blocking_impl::{execute_command, execute_command_with_mode};

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
pub use reloaded_code_bubblewrap::profile as linux_bwrap_profile;
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use reloaded_code_bubblewrap::profile::Profile;

/// Execution mode for bash commands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BashExecutionMode {
    #[default]
    Host,
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    LinuxBwrap(std::sync::Arc<Profile>),
}

/// Serde-friendly bash request owned by the core crate.
#[derive(Debug, Clone, Deserialize)]
pub struct BashRequest {
    /// The shell command to execute.
    pub command: String,
    /// Optional working directory.
    pub workdir: Option<String>,
    /// Timeout in milliseconds. If omitted, uses the tool's default timeout.
    pub timeout_ms: Option<u32>,
}

impl BashRequest {
    /// Parses a raw JSON tool payload into a bash request.
    ///
    /// # Errors
    /// - Returns [`ToolError::Json`] when the JSON payload cannot be deserialized
    ///   into a [`BashRequest`] (e.g., missing `command` field or invalid field types).
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to bash requests.
///
/// When [`BashSettings::permission`] is set, operations may return
/// [`ToolError::PermissionDenied`] if the command is blocked by the ruleset.
#[derive(Debug, Clone, Copy)]
pub struct BashSettings<'a> {
    /// Default timeout when omitted from the request.
    pub default_timeout_ms: u32,
    /// Maximum allowed timeout.
    pub max_timeout_ms: u32,
    /// Default working directory when omitted from the request.
    pub default_workdir: Option<&'a Path>,
    /// Optional permission ruleset applied to command strings.
    ///
    /// When set, blocked commands return [`ToolError::PermissionDenied`].
    pub permission: Option<&'a Ruleset>,
}

/// Default buffer capacity for stdout/stderr pipe reads.
/// 32KB covers typical command output without reallocations.
const PIPE_BUFFER_CAPACITY: usize = 32 * 1024;

#[inline]
fn string_from_utf8_or_lossy(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => match String::from_utf8_lossy(&error.into_bytes()) {
            Cow::Borrowed(text) => text.to_owned(),
            Cow::Owned(text) => text,
        },
    }
}

#[inline]
fn timeout_message_with_buffered_output(
    timeout: Duration,
    stdout_data: &[u8],
    stderr_data: &[u8],
) -> String {
    let stdout = String::from_utf8_lossy(stdout_data);
    let stderr = String::from_utf8_lossy(stderr_data);

    let mut message = String::with_capacity(stdout.len() + stderr.len() + 64);
    let _ = write!(message, "command timed out after {}ms", timeout.as_millis());

    if !stdout.is_empty() {
        message.push('\n');
        message.push_str(&stdout);
    }

    if !stderr.is_empty() {
        if stdout.is_empty() || !stdout.ends_with('\n') {
            message.push('\n');
        }
        message.push_str("[stderr]\n");
        message.push_str(&stderr);
    }

    message
}

#[inline]
fn timeout_error_with_kill_failure(message: String, kill_error: Option<String>) -> ToolError {
    match kill_error {
        Some(kill_error) => ToolError::TimeoutWithKillFailure {
            message,
            kill_error,
        },
        None => ToolError::Timeout(message),
    }
}

#[inline]
fn validate_workdir(workdir: Option<&Path>) -> ToolResult<()> {
    if let Some(dir) = workdir {
        if !dir.is_absolute() {
            return Err(ToolError::InvalidPath(format!(
                "working directory must be an absolute path: {}",
                dir.display()
            )));
        }
        if !dir.is_dir() {
            let msg = if dir.exists() {
                format!("working directory is not a directory: {}", dir.display())
            } else {
                format!("working directory does not exist: {}", dir.display())
            };
            return Err(ToolError::InvalidPath(msg));
        }
    }
    Ok(())
}

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

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
#[inline]
fn map_linux_bwrap_error(error: reloaded_code_bubblewrap::LinuxBwrapError) -> ToolError {
    use reloaded_code_bubblewrap::LinuxBwrapError;
    match error {
        LinuxBwrapError::InvalidPath(message) => ToolError::InvalidPath(message),
        LinuxBwrapError::Execution(message) => ToolError::Execution(message),
    }
}

#[cfg(all(test, feature = "linux-bubblewrap", target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn bwrap_error_mapping_preserves_variants() {
        let mapped = map_linux_bwrap_error(reloaded_code_bubblewrap::LinuxBwrapError::Execution(
            "bwrap missing".to_string(),
        ));
        assert!(matches!(mapped, ToolError::Execution(m) if m.contains("bwrap")));

        let mapped = map_linux_bwrap_error(reloaded_code_bubblewrap::LinuxBwrapError::InvalidPath(
            "bad path".to_string(),
        ));
        assert!(matches!(mapped, ToolError::InvalidPath(m) if m.contains("bad")));
    }
}
