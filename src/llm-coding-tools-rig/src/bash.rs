//! Shell command execution tool.
//!
//! Provides cross-platform shell command execution with timeout support.

use llm_coding_tools_core::operations::execute_command;
use llm_coding_tools_core::{BashOutput, ToolContext, ToolError, ToolOutput};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

/// Default timeout: 2 minutes.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// Arguments for the bash tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BashArgs {
    /// The shell command to execute.
    pub command: String,
    /// Optional working directory (must be absolute path).
    pub workdir: Option<String>,
    /// Timeout in milliseconds (default: 120000).
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

/// Tool for executing shell commands.
///
/// Uses bash on Unix, cmd on Windows.
#[derive(Debug, Clone, Copy, Default)]
pub struct BashTool;

impl BashTool {
    /// Creates a new bash tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Tool for BashTool {
    const NAME: &'static str = "bash";

    type Error = ToolError;
    type Args = BashArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Execute a shell command with optional working directory and timeout."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(BashArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let workdir = args.workdir.as_ref().map(Path::new);
        let timeout = Duration::from_millis(args.timeout_ms);

        let result = execute_command(&args.command, workdir, timeout).await?;
        Ok(format_bash_output(&result))
    }
}

impl ToolContext for BashTool {
    const NAME: &'static str = "bash";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::BASH
    }
}

fn format_bash_output(output: &BashOutput) -> ToolOutput {
    let mut content = String::new();

    if !output.stdout.is_empty() {
        content.push_str(&output.stdout);
    }
    if !output.stderr.is_empty() {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[stderr]\n");
        content.push_str(&output.stderr);
    }

    if let Some(code) = output.exit_code {
        if code != 0 {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&format!("[exit code: {}]", code));
        }
    }

    ToolOutput::new(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn executes_echo() {
        let tool = BashTool::new();
        let args = BashArgs {
            command: "echo hello".to_string(),
            workdir: None,
            timeout_ms: 5000,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.content.contains("hello"));
    }

    #[tokio::test]
    async fn timeout_returns_error() {
        let tool = BashTool::new();
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };
        let args = BashArgs {
            command: cmd.to_string(),
            workdir: None,
            timeout_ms: 100,
        };
        let result = tool.call(args).await;
        assert!(matches!(result, Err(ToolError::Timeout(_))));
    }
}
