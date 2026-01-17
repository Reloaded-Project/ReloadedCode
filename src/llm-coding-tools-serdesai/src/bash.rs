//! Shell command execution tool.
//!
//! Provides cross-platform shell command execution with timeout support.

use crate::convert::to_serdes_result;
use async_trait::async_trait;
use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::operations::execute_command;
use llm_coding_tools_core::tool_names;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default timeout: 2 minutes.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

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
#[derive(Debug, Clone, Default)]
pub struct BashTool {
    /// Default timeout for commands when not specified in args.
    default_timeout: Option<Duration>,
    /// Default working directory when not specified in args.
    default_workdir: Option<PathBuf>,
}

impl BashTool {
    /// Creates a new bash tool instance with default settings.
    #[inline]
    pub fn new() -> Self {
        Self::default()
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
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            tool_names::BASH,
            "Execute a shell command with optional working directory and timeout.",
        )
        .with_parameters(
            SchemaBuilder::new()
                .string_constrained(
                    "command",
                    "The shell command to execute",
                    true,
                    Some(1),
                    None,
                    None,
                )
                .string(
                    "workdir",
                    "Working directory for command execution (must be absolute path)",
                    false,
                )
                .integer_constrained(
                    "timeout_ms",
                    "Timeout in milliseconds. Defaults to 120000 (2 minutes).",
                    false,
                    Some(1),
                    Some(600_000),
                )
                .build()
                .expect("schema serialization should never fail"),
        )
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: BashArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::BASH, None, e.to_string()))?;

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
            .unwrap_or(Duration::from_millis(DEFAULT_TIMEOUT_MS));

        let result = execute_command(&args.command, workdir, timeout).await;

        to_serdes_result(
            tool_names::BASH,
            result.map(|output| output.format_output()),
        )
    }
}

impl ToolContext for BashTool {
    const NAME: &'static str = tool_names::BASH;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::BASH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_ctx() -> RunContext<()> {
        RunContext::minimal("test-model")
    }

    #[tokio::test]
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

    /// Canonicalizes a path for comparison (resolves symlinks like /tmp -> /private/tmp on macOS).
    #[cfg(not(windows))]
    fn normalize_for_comparison(path: std::path::PathBuf) -> String {
        path.canonicalize()
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    /// Canonicalizes a path for comparison, stripping Windows' `\\?\` extended-length prefix.
    #[cfg(windows)]
    fn normalize_for_comparison(path: std::path::PathBuf) -> String {
        let canonical = path.canonicalize().unwrap_or(path);
        let s = canonical.to_string_lossy();
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    }

    #[tokio::test]
    async fn workdir_parameter_changes_directory() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.to_string_lossy().to_string();
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
        // Canonicalize and normalize path for comparison
        let expected = normalize_for_comparison(temp_dir);
        assert!(output.contains(&expected));
    }

    #[tokio::test]
    async fn default_workdir_is_used() {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.to_string_lossy().to_string();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };
        let tool = BashTool::new().with_default_workdir(&temp_path);
        let args = serde_json::json!({
            "command": cmd
        });
        let result = tool.call(&mock_ctx(), args).await.unwrap();
        let output = result.as_text().unwrap();
        // Canonicalize and normalize path for comparison
        let expected = normalize_for_comparison(temp_dir);
        assert!(output.contains(&expected));
    }

    #[tokio::test]
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
}
