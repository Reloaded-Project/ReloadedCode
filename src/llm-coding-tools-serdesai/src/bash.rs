//! Shell command execution tool.
//!
//! # Public API
//!
//! - [`BashTool::host`] — runs commands directly on the host shell.
//! - [`BashTool::new`] — backward-compatible alias for [`BashTool::host`].
#![cfg_attr(
    all(feature = "linux-bubblewrap", target_os = "linux"),
    doc = "\
        - [`BashTool::with_linux_bwrap`] — runs commands inside a Linux bubblewrap sandbox.\n\
        \n\
        # Linux Sandbox Profiles\n\
        \n\
        On Linux with the `linux-bubblewrap` feature, commands can run \
        inside a bubblewrap sandbox. Two profile presets are available:\n\
        \n\
        - [`Builder::public_bot`](crate::profile::Builder::public_bot) — \
          strict isolation for untrusted input.\n\
        - [`Builder::trusted_maintenance`](crate::profile::Builder::trusted_maintenance) — \
          looser sandbox for build automation. Not safe against hostile commands.\n\
        \n\
        See the workspace guide at \
        <https://github.com/Sewer56/llm-coding-tools/blob/main/SANDBOX-PROFILES.md> \
        for full profile configuration and setup instructions."
)]

use crate::convert::to_serdes_result;
use async_trait::async_trait;
use llm_coding_tools_core::context::{ToolContext, ToolPrompt};
use llm_coding_tools_core::tool_metadata::bash as bash_meta;
use llm_coding_tools_core::tools::{BashExecutionMode, execute_command_with_mode};
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use llm_coding_tools_bubblewrap::profile::{NetworkPolicy, Profile};

/// Arguments for the bash tool.
#[derive(Debug, Clone, Deserialize)]
struct BashArgs {
    /// The shell command to execute.
    command: String,
    /// Optional working directory (must be absolute path).
    workdir: Option<String>,
    /// Timeout in milliseconds. Optional - falls back to constructor default or 120000ms.
    timeout_ms: Option<u64>,
}

/// Tool for executing shell commands.
///
/// Uses bash on Unix, cmd on Windows.
#[derive(Debug, Clone)]
pub struct BashTool {
    /// Explicit execution mode for this tool instance.
    mode: BashExecutionMode, // ZST. 0 bytes when all optionals disabled.
    /// Default timeout for commands when not specified in args.
    default_timeout: Option<Duration>,
    /// Default working directory when not specified in args.
    default_workdir: Option<PathBuf>,
}

impl Default for BashTool {
    fn default() -> Self {
        Self::host()
    }
}

impl BashTool {
    /// Creates a new bash tool instance with default settings.
    ///
    /// This is an alias for [`Self::host`] for backward compatibility.
    /// Prefer [`Self::host`] in examples so host execution stays explicit.
    #[inline]
    pub fn new() -> Self {
        Self::host()
    }

    /// Creates a bash tool that runs commands directly on the host shell.
    /// On Linux with the `linux-bubblewrap` feature, call `with_linux_bwrap` instead
    /// to sandbox commands.
    pub fn host() -> Self {
        Self {
            mode: BashExecutionMode::Host,
            default_timeout: None,
            default_workdir: None,
        }
    }

    /// Returns the configured execution mode.
    pub fn mode(&self) -> &BashExecutionMode {
        &self.mode
    }

    /// Sets the default timeout for commands.
    ///
    /// This timeout is used when `timeout_ms` is not provided in the tool arguments.
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    /// Sets the default working directory.
    ///
    /// This directory is used when `workdir` is not provided in the tool arguments.
    pub fn with_default_workdir(mut self, workdir: impl Into<PathBuf>) -> Self {
        self.default_workdir = Some(workdir.into());
        self
    }

    /// Runs commands inside a Linux sandbox using bubblewrap.
    ///
    /// Accepts an owned [`Profile`] or `Arc<Profile>` to share one profile across
    /// multiple tool instances.
    ///
    /// Build a profile with [`crate::profile::Builder::public_bot`] for untrusted input
    /// or [`crate::profile::Builder::trusted_maintenance`] for build automation that
    /// needs network access. Call [`crate::profile::Availability::detect`] at startup to
    /// verify the sandbox is usable.
    ///
    /// # Platform
    ///
    /// Only available on Linux with the `linux-bubblewrap` feature enabled.
    ///
    /// # Warnings
    ///
    /// Trusted-maintenance profiles allow network access and are not safe against
    /// hostile commands. Pass only short-lived tokens via `with_extra_env` and
    /// job-scoped read-only files via `with_credential_file_mounts`. Do not forward
    /// SSH agents or mount full host credential stores.
    ///
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    pub fn with_linux_bwrap(mut self, profile: impl Into<std::sync::Arc<Profile>>) -> Self {
        self.mode = BashExecutionMode::LinuxBwrap(profile.into());
        self
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(bash_meta::NAME, bash_meta::DESCRIPTION).with_parameters(
            SchemaBuilder::new()
                .string_constrained(
                    bash_meta::param::COMMAND.name,
                    bash_meta::param::COMMAND.description,
                    bash_meta::param::COMMAND.required,
                    Some(1),
                    None,
                    None,
                )
                .string(
                    bash_meta::param::WORKDIR.name,
                    bash_meta::param::WORKDIR.description,
                    bash_meta::param::WORKDIR.required,
                )
                .integer_constrained(
                    bash_meta::param::TIMEOUT_MS.name,
                    bash_meta::param::TIMEOUT_MS.description,
                    bash_meta::param::TIMEOUT_MS.required,
                    Some(1),
                    Some(bash_meta::MAX_TIMEOUT_MS as i64),
                )
                .build()
                .expect("schema serialization should never fail"),
        )
    }

    /// Executes a shell command through the configured [`BashExecutionMode`].
    ///
    /// # Errors
    ///
    /// - [`ToolError::ValidationFailed`] if the JSON arguments fail deserialization.
    /// - [`ToolError::ExecutionFailed`] if the command cannot be spawned, the per-command
    ///   workdir is invalid, or a timeout or I/O failure occurs while collecting
    ///   output.
    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: BashArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(bash_meta::NAME, None, e.to_string()))?;

        // Use arg workdir, falling back to default_workdir
        let workdir: Option<&Path> = args
            .workdir
            .as_ref()
            .map(|s| Path::new(s.as_str()))
            .or(self.default_workdir.as_deref());

        // Priority: args.timeout_ms > self.default_timeout > DEFAULT_TIMEOUT_MS
        let timeout = args
            .timeout_ms
            .map(Duration::from_millis)
            .or(self.default_timeout)
            .unwrap_or(Duration::from_millis(bash_meta::DEFAULT_TIMEOUT_MS));

        // Route execution through mode-aware entrypoint to honor explicit mode selection
        let result = execute_command_with_mode(&self.mode, &args.command, workdir, timeout).await;

        to_serdes_result(bash_meta::NAME, result.map(|output| output.format_output()))
    }
}

#[inline]
fn bash_prompt_network_disabled(mode: &BashExecutionMode) -> bool {
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    {
        matches!(
            mode,
            BashExecutionMode::LinuxBwrap(config)
                if matches!(config.network_policy(), NetworkPolicy::Disabled)
        )
    }

    #[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
    {
        let _ = mode;
        false
    }
}

#[inline]
fn bash_prompt_sandboxed(mode: &BashExecutionMode) -> bool {
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    {
        matches!(mode, BashExecutionMode::LinuxBwrap(_))
    }

    #[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
    {
        let _ = mode;
        false
    }
}

impl ToolContext for BashTool {
    const NAME: &'static str = bash_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Bash {
            network_disabled: bash_prompt_network_disabled(&self.mode),
            sandboxed: bash_prompt_sandboxed(&self.mode),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    fn mock_ctx() -> RunContext<()> {
        RunContext::minimal("test-model")
    }

    #[tokio::test]
    #[serial]
    async fn executes_echo() {
        let tool = BashTool::new();
        let args = serde_json::json!({
            "command": "echo hello",
            "timeout_ms": 5000
        });
        let result = tool.call(&mock_ctx(), args).await.unwrap();
        assert!(result.as_text().unwrap().contains("hello"));
    }

    #[tokio::test]
    #[serial]
    async fn timeout_returns_error() {
        let tool = BashTool::new();
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        let args = serde_json::json!({
            "command": cmd,
            "timeout_ms": 100
        });
        let result = tool.call(&mock_ctx(), args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn workdir_parameter_changes_directory() {
        let temp = tempfile::TempDir::new().unwrap();
        let temp_path = temp.path().to_string_lossy();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };
        let tool = BashTool::new();
        let args = serde_json::json!({
            "command": cmd,
            "workdir": temp_path,
            "timeout_ms": 5000
        });
        let result = tool.call(&mock_ctx(), args).await.unwrap();
        let output = result.as_text().unwrap();
        assert!(output.contains(temp_path.as_ref()));
    }

    #[tokio::test]
    #[serial]
    async fn default_workdir_is_used() {
        let temp = tempfile::TempDir::new().unwrap();
        let temp_path = temp.path().to_string_lossy();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };
        let tool = BashTool::new().with_default_workdir(temp_path.as_ref());
        let args = serde_json::json!({
            "command": cmd
        });
        let result = tool.call(&mock_ctx(), args).await.unwrap();
        let output = result.as_text().unwrap();
        assert!(output.contains(temp_path.as_ref()));
    }

    #[tokio::test]
    #[serial]
    async fn per_call_timeout_overrides_default() {
        // Constructor sets 10s default, but per-call arg specifies 100ms
        let tool = BashTool::new().with_default_timeout(Duration::from_secs(10));
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        let args = serde_json::json!({
            "command": cmd,
            "timeout_ms": 100  // Should override the 10s default
        });
        let result = tool.call(&mock_ctx(), args).await;
        // Should timeout with the 100ms, not wait 10s
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn default_timeout_used_when_arg_omitted() {
        let tool = BashTool::new().with_default_timeout(Duration::from_millis(100));
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        // No timeout_ms in args - should use constructor default
        let args = serde_json::json!({
            "command": cmd
        });
        let result = tool.call(&mock_ctx(), args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn new_reports_host_mode_by_default() {
        let tool = BashTool::new();
        assert!(matches!(tool.mode(), BashExecutionMode::Host));
    }

    #[tokio::test]
    #[serial]
    async fn bash_context_reports_host_mode() {
        use llm_coding_tools_core::context::ToolPrompt;

        let host_tool = BashTool::new();
        assert!(
            matches!(
                host_tool.context(),
                ToolPrompt::Bash {
                    network_disabled: false,
                    sandboxed: false,
                }
            ),
            "host bash should report network_disabled: false, sandboxed: false"
        );
    }
}
