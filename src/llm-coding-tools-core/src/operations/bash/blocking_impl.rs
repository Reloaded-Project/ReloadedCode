//! Blocking shell command execution.

use super::{BashOutput, PIPE_BUFFER_CAPACITY};
use crate::error::{ToolError, ToolResult};
use process_wrap::std::*;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

/// Executes a shell command with optional working directory and timeout.
///
/// Uses bash on Unix, cmd on Windows. Process tree is killed on timeout via:
/// - Windows: Job Objects
/// - Unix: Process groups
pub fn execute_command(
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

    let mut child = wrap
        .spawn()
        .map_err(|e| ToolError::Execution(e.to_string()))?;

    // Take stdout/stderr handles to drain them in separate threads.
    // If the child produces more output than the pipe buffer can hold (~64KB on
    // Linux, ~4KB on Windows), it blocks waiting for the parent to read. Without
    // concurrent draining, the child would never exit and we'd always hit the
    // timeout. By draining pipes concurrently with polling try_wait(), the child
    // can always make progress.
    let mut stdout_handle = child.stdout().take().expect("stdout was piped");
    let mut stderr_handle = child.stderr().take().expect("stderr was piped");

    // Spawn threads to drain stdout/stderr concurrently
    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::with_capacity(PIPE_BUFFER_CAPACITY);
        let _ = stdout_handle.read_to_end(&mut buf);
        buf
    });

    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::with_capacity(PIPE_BUFFER_CAPACITY);
        let _ = stderr_handle.read_to_end(&mut buf);
        buf
    });

    let start = Instant::now();

    // Poll for completion with timeout
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // Kill entire process tree via Job Object (Windows) or process group (Unix)
                    let _ = child.kill();
                    break Err(ToolError::Timeout(format!(
                        "command timed out after {}ms",
                        timeout.as_millis()
                    )));
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => break Err(ToolError::Execution(e.to_string())),
        }
    };

    // Join pipe-draining threads (they will complete once child exits or is killed)
    let stdout_data = stdout_thread.join().unwrap_or_default();
    let stderr_data = stderr_thread.join().unwrap_or_default();

    // Return result
    match exit_status {
        Ok(status) => Ok(BashOutput {
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&stdout_data).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_data).into_owned(),
        }),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn execute_echo_returns_output() {
        let result = execute_command("echo hello", None, Duration::from_secs(5)).unwrap();

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn respects_working_directory() {
        let temp = TempDir::new().unwrap();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };

        let result = execute_command(cmd, Some(temp.path()), Duration::from_secs(5)).unwrap();

        assert_eq!(result.exit_code, Some(0));
        let temp_path = temp.path().to_string_lossy();
        assert!(result.stdout.contains(temp_path.as_ref()));
    }

    #[test]
    fn timeout_returns_error() {
        let cmd = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1"
        } else {
            "sleep 10"
        };

        let result = execute_command(cmd, None, Duration::from_millis(100));
        assert!(matches!(result, Err(ToolError::Timeout(_))));
    }

    #[test]
    fn invalid_workdir_returns_error() {
        let result = execute_command(
            "echo hello",
            Some(Path::new("/nonexistent/path")),
            Duration::from_secs(5),
        );

        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }

    #[test]
    fn captures_exit_code() {
        let cmd = if cfg!(target_os = "windows") {
            "exit /b 42"
        } else {
            "exit 42"
        };

        let result = execute_command(cmd, None, Duration::from_secs(5)).unwrap();

        assert_eq!(result.exit_code, Some(42));
    }

    /// Test that large output (exceeding pipe buffer) doesn't deadlock.
    /// Pipe buffers are typically 64KB on Linux, 4KB on Windows.
    /// This test would hang/timeout with the old implementation that
    /// waited for process exit before reading pipes.
    #[test]
    fn large_output_does_not_deadlock() {
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
            // type command on Windows
            format!("type {}", large_file.display())
        } else {
            format!("cat {}", large_file.display())
        };

        let result = execute_command(&cmd, None, Duration::from_secs(30)).unwrap();

        assert_eq!(result.exit_code, Some(0));
        // Verify we got all the output (102400 bytes written)
        assert_eq!(result.stdout.len(), 102400);
    }
}
