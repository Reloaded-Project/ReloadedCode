//! Async shell command execution.

use super::BashOutput;
use crate::error::{ToolError, ToolResult};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Executes a shell command with optional working directory and timeout.
///
/// Uses bash on Unix, cmd on Windows.
pub async fn execute_command(
    command: &str,
    workdir: Option<&Path>,
    timeout: Duration,
) -> ToolResult<BashOutput> {
    if let Some(dir) = workdir {
        if !dir.is_absolute() {
            return Err(ToolError::InvalidPath(format!(
                "working directory must be an absolute path: {}",
                dir.display()
            )));
        }
        if !dir.is_dir() {
            return Err(ToolError::InvalidPath(format!(
                "working directory does not exist: {}",
                dir.display()
            )));
        }
    }

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("bash");
        c.args(["-c", command]);
        c
    };

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let result = tokio::time::timeout(timeout, cmd.output()).await;

    match result {
        Ok(Ok(output)) => Ok(BashOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
        Ok(Err(e)) => Err(ToolError::Execution(e.to_string())),
        Err(_) => Err(ToolError::Timeout(format!(
            "command timed out after {}ms",
            timeout.as_millis()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn execute_echo_returns_output() {
        let result = execute_command("echo hello", None, Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn respects_working_directory() {
        let temp = TempDir::new().unwrap();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };

        let result = execute_command(cmd, Some(temp.path()), Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(result.exit_code, Some(0));
        let temp_path = temp.path().to_string_lossy();
        assert!(result.stdout.contains(temp_path.as_ref()));
    }

    #[tokio::test]
    async fn timeout_returns_error() {
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };

        let result = execute_command(cmd, None, Duration::from_millis(100)).await;
        assert!(matches!(result, Err(ToolError::Timeout(_))));
    }

    #[tokio::test]
    async fn invalid_workdir_returns_error() {
        let result = execute_command(
            "echo hello",
            Some(Path::new("/nonexistent/path")),
            Duration::from_secs(5),
        )
        .await;

        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }

    #[tokio::test]
    async fn captures_exit_code() {
        let cmd = if cfg!(target_os = "windows") {
            "exit /b 42"
        } else {
            "exit 42"
        };

        let result = execute_command(cmd, None, Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(result.exit_code, Some(42));
    }
}
