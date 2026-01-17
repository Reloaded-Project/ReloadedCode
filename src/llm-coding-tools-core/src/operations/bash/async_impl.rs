//! Async shell command execution.

use super::BashOutput;
use crate::error::{ToolError, ToolResult};
use process_wrap::tokio::*;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// Executes a shell command with optional working directory and timeout.
///
/// Uses bash on Unix, cmd on Windows. Process tree is killed on timeout via:
/// - Windows: Job Objects
/// - Unix: Process groups
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

    #[cfg(windows)]
    let mut wrap = CommandWrap::with_new("cmd", |cmd| {
        cmd.args(["/C", command]);
        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });

    #[cfg(not(windows))]
    let mut wrap = CommandWrap::with_new("bash", |cmd| {
        cmd.args(["-c", command]);
        if let Some(dir) = workdir {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });

    // Add platform-specific process tree management
    #[cfg(windows)]
    wrap.wrap(JobObject);
    #[cfg(unix)]
    wrap.wrap(ProcessGroup::leader());

    let mut child: Box<dyn ChildWrapper> = wrap
        .spawn()
        .map_err(|e| ToolError::Execution(e.to_string()))?;

    // Take stdout/stderr handles to drain them in separate tasks.
    // This prevents deadlock when output exceeds pipe buffer (~64KB Linux, ~4KB Windows).
    // We keep the child handle available so we can call kill() on timeout.
    let mut stdout_pipe = child.stdout().take();
    let mut stderr_pipe = child.stderr().take();

    // Spawn tasks to drain pipes concurrently
    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(ref mut io) = stdout_pipe {
            let _ = io.read_to_end(&mut buf).await;
        }
        buf
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        if let Some(ref mut io) = stderr_pipe {
            let _ = io.read_to_end(&mut buf).await;
        }
        buf
    });

    // Race between timeout and process completion
    tokio::select! {
        biased;  // Check timeout first for consistent behavior

        _ = tokio::time::sleep(timeout) => {
            // Timeout: explicitly kill the process tree (Job Object on Windows, process group on Unix)
            let _ = Pin::from(child.kill()).await;
            Err(ToolError::Timeout(format!(
                "command timed out after {}ms",
                timeout.as_millis()
            )))
        }

        status = child.wait() => {
            let status = status.map_err(|e| ToolError::Execution(e.to_string()))?;

            // Join pipe-draining tasks (they complete once child exits or is killed)
            let stdout_data = stdout_task.await.unwrap_or_default();
            let stderr_data = stderr_task.await.unwrap_or_default();

            Ok(BashOutput {
                exit_code: status.code(),
                stdout: String::from_utf8_lossy(&stdout_data).into_owned(),
                stderr: String::from_utf8_lossy(&stderr_data).into_owned(),
            })
        }
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

    /// Test that large output (exceeding pipe buffer) doesn't deadlock.
    /// Pipe buffers are typically 64KB on Linux, 4KB on Windows.
    /// This test would hang/timeout with the old implementation that
    /// waited for process exit before reading pipes.
    #[tokio::test]
    async fn large_output_does_not_deadlock() {
        use std::io::Write;

        // Create a temp file with large content, then cat/type it
        // Use tempfile::Builder to create directory without dot prefix
        let temp_dir = tempfile::Builder::new()
            .prefix("llmtest")
            .tempdir()
            .unwrap();
        let large_file = temp_dir.path().join("large.txt");
        {
            let mut file = std::fs::File::create(&large_file).unwrap();
            // Write 100KB of 'x' characters (single line to avoid newline issues)
            let content = "x".repeat(102400);
            file.write_all(content.as_bytes()).unwrap();
        }

        let cmd = if cfg!(target_os = "windows") {
            // type command on Windows - path without quotes, use short 8.3 name if needed
            format!("type {}", large_file.display())
        } else {
            format!("cat {}", large_file.display())
        };

        let result = execute_command(&cmd, None, Duration::from_secs(30))
            .await
            .unwrap();

        assert_eq!(result.exit_code, Some(0));
        // Verify we got all the output (102400 bytes written)
        assert_eq!(result.stdout.len(), 102400);
    }
}
