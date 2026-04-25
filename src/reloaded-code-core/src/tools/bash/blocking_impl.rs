//! Blocking shell command execution.

use super::{
    string_from_utf8_or_lossy, timeout_error_with_kill_failure,
    timeout_message_with_buffered_output, validate_workdir, BashExecutionMode, BashOutput,
    PIPE_BUFFER_CAPACITY,
};
use crate::error::{ToolError, ToolResult};
use crate::permissions_ext::OptionRulesetExt;
use crate::tool_metadata::bash as bash_meta;
use process_wrap::std::*;
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use reloaded_code_bubblewrap::wrap::blocking as linux_bwrap_wrap;
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};

/// Initial sleep between non-blocking wait polls.
const INITIAL_POLL_INTERVAL_MS: u64 = 1;
/// Upper bound for wait poll backoff.
const MAX_POLL_INTERVAL_MS: u64 = 10;

enum WaitOutcome {
    Exited(std::process::ExitStatus),
    TimedOut { kill_error: Option<std::io::Error> },
    WaitError(std::io::Error),
}

/// Executes a shell command with optional working directory and timeout.
///
/// Uses bash on Unix, cmd on Windows. Process tree is killed on timeout via:
/// - Windows: Job Objects
/// - Unix: Process groups
///
/// # Errors
/// - Returns [`ToolError::PermissionDenied`] when the command is blocked by `settings.permission`.
/// - Returns `ToolError::Validation` if timeout is 0 or exceeds max_timeout_ms.
/// - Returns [`ToolError::InvalidPath`] if workdir is not absolute or doesn't exist.
/// - Returns [`ToolError::Execution`] for sandbox mode when bwrap is missing or unusable.
/// - Returns [`ToolError::Timeout`] or [`ToolError::TimeoutWithKillFailure`] on timeout.
pub fn execute_command(
    mode: &BashExecutionMode,
    request: super::BashRequest,
    settings: super::BashSettings<'_>,
) -> ToolResult<BashOutput> {
    settings
        .permission
        .check(bash_meta::NAME, &request.command)?;

    let workdir = request
        .workdir
        .as_deref()
        .map(Path::new)
        .or(settings.default_workdir);
    let timeout_ms = request.timeout_ms.unwrap_or(settings.default_timeout_ms);

    execute_command_with_mode(
        mode,
        &request.command,
        workdir,
        timeout_ms,
        settings.max_timeout_ms,
    )
}

/// Executes a shell command with explicit mode selection.
///
/// # Arguments
/// - `mode` - The execution mode (host or Linux sandbox).
/// - `command` - The shell command to execute.
/// - `workdir` - Optional working directory (must be absolute if provided).
/// - `timeout_ms` - Timeout in milliseconds (must be >= 1 and <= max_timeout_ms).
/// - `max_timeout_ms` - Maximum allowed timeout in milliseconds.
///
/// # Errors
/// - Returns `ToolError::Validation` if timeout_ms is 0 or exceeds max_timeout_ms.
/// - Returns [`ToolError::InvalidPath`] if workdir is not absolute or doesn't exist.
/// - Returns [`ToolError::Execution`] for sandbox mode when bwrap is missing or unusable.
/// - Returns [`ToolError::Timeout`] or [`ToolError::TimeoutWithKillFailure`] on timeout.
pub fn execute_command_with_mode(
    mode: &BashExecutionMode,
    command: &str,
    workdir: Option<&Path>,
    timeout_ms: u32,
    max_timeout_ms: u32,
) -> ToolResult<BashOutput> {
    // Validate timeout_ms
    if timeout_ms == 0 {
        return Err(ToolError::validation_for(
            "timeout_ms",
            "timeout_ms must be at least 1",
        ));
    }
    if timeout_ms > max_timeout_ms {
        return Err(ToolError::validation_for(
            "timeout_ms",
            format!(
                "timeout_ms exceeds maximum allowed value of {}",
                max_timeout_ms
            ),
        ));
    }

    let timeout = Duration::from_millis(timeout_ms as u64);
    let wrap = match mode {
        BashExecutionMode::Host => build_host_wrap(command, workdir),
        #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
        BashExecutionMode::LinuxBwrap(config) => {
            linux_bwrap_wrap::build_command_wrap(config, command, workdir)
                .map_err(super::map_linux_bwrap_error)
        }
    }?;
    run_wrapped_command(wrap, timeout)
}

/// Runs a wrapped command with timeout, concurrent pipe draining, and proper cleanup.
///
/// This is the shared implementation for both host and sandbox execution in blocking mode.
pub(in crate::tools::bash) fn run_wrapped_command(
    mut wrap: CommandWrap,
    timeout: Duration,
) -> ToolResult<BashOutput> {
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
    let mut poll_interval_ms = INITIAL_POLL_INTERVAL_MS;

    // Poll for completion with timeout.
    let wait_outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break WaitOutcome::Exited(status),
            Ok(None) => {
                let elapsed = start.elapsed();
                if elapsed >= timeout {
                    // Kill entire process tree via Job Object (Windows) or process group (Unix)
                    break WaitOutcome::TimedOut {
                        kill_error: child.kill().err(),
                    };
                }

                let remaining = timeout.saturating_sub(elapsed);
                thread::sleep(remaining.min(Duration::from_millis(poll_interval_ms)));
                poll_interval_ms = (poll_interval_ms << 1).min(MAX_POLL_INTERVAL_MS);
            }
            Err(e) => break WaitOutcome::WaitError(e),
        }
    };

    // Join pipe-draining threads (they will complete once child exits or is killed)
    let stdout_data = stdout_thread
        .join()
        .map_err(|_| ToolError::Execution("stdout reader thread panicked".to_string()))?;
    let stderr_data = stderr_thread
        .join()
        .map_err(|_| ToolError::Execution("stderr reader thread panicked".to_string()))?;

    // Return result
    match wait_outcome {
        WaitOutcome::Exited(status) => Ok(BashOutput {
            exit_code: status.code(),
            stdout: string_from_utf8_or_lossy(stdout_data),
            stderr: string_from_utf8_or_lossy(stderr_data),
        }),
        WaitOutcome::TimedOut { kill_error } => Err(timeout_error_with_kill_failure(
            timeout_message_with_buffered_output(timeout, &stdout_data, &stderr_data),
            kill_error.map(|e| e.to_string()),
        )),
        WaitOutcome::WaitError(e) => Err(ToolError::Execution(e.to_string())),
    }
}

fn build_host_wrap(command: &str, workdir: Option<&Path>) -> ToolResult<CommandWrap> {
    validate_workdir(workdir)?;

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

    #[cfg(windows)]
    wrap.wrap(JobObject);
    #[cfg(unix)]
    wrap.wrap(ProcessGroup::leader());

    Ok(wrap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{ExpandError, PermissionAction, Rule, Ruleset};
    use crate::tool_metadata::bash as bash_meta;
    use crate::tools::{BashRequest, BashSettings};
    use tempfile::TempDir;

    type TestResult = Result<(), ExpandError>;

    #[test]
    fn execute_echo_returns_output() {
        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: "echo hello".to_string(),
                workdir: None,
                timeout_ms: None,
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: None,
            },
        )
        .unwrap();

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn rejected_command_returns_permission_denied() -> TestResult {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new(bash_meta::NAME, "*", PermissionAction::Allow)?);
        ruleset.push(Rule::new(
            bash_meta::NAME,
            "echo hello",
            PermissionAction::Deny,
        )?);

        let err = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: "echo hello".to_string(),
                workdir: None,
                timeout_ms: None,
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: Some(&ruleset),
            },
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ToolError::PermissionDenied { tool: "bash", .. }
        ));
        Ok(())
    }

    #[test]
    fn respects_working_directory() {
        let temp = TempDir::new().unwrap();
        let cmd = if cfg!(target_os = "windows") {
            "cd"
        } else {
            "pwd"
        };

        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: cmd.to_string(),
                workdir: Some(temp.path().to_str().unwrap().to_string()),
                timeout_ms: None,
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: None,
            },
        )
        .unwrap();

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

        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: cmd.to_string(),
                workdir: None,
                timeout_ms: Some(100),
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: None,
            },
        );
        assert!(matches!(
            result,
            Err(ToolError::Timeout(_) | ToolError::TimeoutWithKillFailure { .. })
        ));
    }

    #[test]
    fn invalid_workdir_returns_error() {
        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: "echo hello".to_string(),
                workdir: Some("/nonexistent/path".to_string()),
                timeout_ms: None,
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: None,
            },
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

        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: cmd.to_string(),
                workdir: None,
                timeout_ms: None,
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: None,
            },
        )
        .unwrap();

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

        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: cmd,
                workdir: None,
                timeout_ms: Some(30000),
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 60000,
                default_workdir: None,
                permission: None,
            },
        )
        .unwrap();

        assert_eq!(result.exit_code, Some(0));
        // Verify we got all the output (102400 bytes written)
        assert_eq!(result.stdout.len(), 102400);
    }
}
