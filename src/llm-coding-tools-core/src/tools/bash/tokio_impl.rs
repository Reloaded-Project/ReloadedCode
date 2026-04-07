//! Tokio-based async shell command execution.

use super::{
    string_from_utf8_or_lossy, timeout_error_with_kill_failure,
    timeout_message_with_buffered_output, validate_workdir, BashExecutionMode, BashOutput,
    PIPE_BUFFER_CAPACITY,
};
use crate::error::{ToolError, ToolResult};
use crate::permissions_ext::OptionRulesetExt;
use crate::tool_metadata::bash as bash_meta;
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use llm_coding_tools_bubblewrap::wrap::tokio as linux_bwrap_wrap;
use parking_lot::Mutex;
use process_wrap::tokio::*;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;

/// Maximum time to wait for pipe drains after timeout kill.
const PIPE_DRAIN_GRACE_PERIOD: Duration = Duration::from_millis(100);
/// Read chunk size for async pipe draining.
const PIPE_DRAIN_READ_CHUNK: usize = 8 * 1024;

type SharedPipeBuffer = Arc<Mutex<Vec<u8>>>;

struct PipeDrainTask {
    handle: JoinHandle<()>,
    buffer: SharedPipeBuffer,
}

#[inline]
fn spawn_pipe_drain_task<R>(mut pipe: R) -> PipeDrainTask
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let buffer: SharedPipeBuffer = Arc::new(Mutex::new(Vec::with_capacity(PIPE_BUFFER_CAPACITY)));
    let task_buffer = Arc::clone(&buffer);

    let handle = tokio::spawn(async move {
        let mut chunk = [0_u8; PIPE_DRAIN_READ_CHUNK];
        loop {
            match pipe.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => task_buffer.lock().extend_from_slice(&chunk[..read]),
                Err(_) => break,
            }
        }
    });

    PipeDrainTask { handle, buffer }
}

#[inline]
fn take_pipe_buffer(buffer: SharedPipeBuffer) -> Vec<u8> {
    match Arc::try_unwrap(buffer) {
        Ok(mutex) => mutex.into_inner(),
        Err(shared) => core::mem::take(&mut *shared.lock()),
    }
}

#[inline]
async fn await_pipe_drain_task(task: PipeDrainTask) -> Vec<u8> {
    let PipeDrainTask { handle, buffer } = task;
    let _ = handle.await;
    take_pipe_buffer(buffer)
}

#[inline]
async fn await_pipe_drain_task_with_grace(task: PipeDrainTask, grace: Duration) -> Vec<u8> {
    let PipeDrainTask { mut handle, buffer } = task;

    tokio::select! {
        _ = &mut handle => {},
        _ = tokio::time::sleep(grace) => {
            // Preserve strict timeout semantics while retaining buffered bytes.
            // Buffer state is shared outside the task so abort cannot discard it.
            handle.abort();
            let _ = handle.await;
        }
    }

    take_pipe_buffer(buffer)
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
pub async fn execute_command(
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
    .await
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
pub async fn execute_command_with_mode(
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
    run_wrapped_command(wrap, timeout).await
}

/// Runs a wrapped command with timeout, concurrent pipe draining, and proper cleanup.
///
/// This is the shared implementation for both host and sandbox execution on tokio.
pub(in crate::tools::bash) async fn run_wrapped_command(
    mut wrap: CommandWrap,
    timeout: Duration,
) -> ToolResult<BashOutput> {
    let mut child: Box<dyn ChildWrapper> = wrap
        .spawn()
        .map_err(|e| ToolError::Execution(e.to_string()))?;

    // Take stdout/stderr handles to drain them concurrently with process wait.
    // This prevents deadlock when output exceeds pipe buffer (~64KB Linux, ~4KB Windows).
    let stdout_pipe = child.stdout().take().expect("stdout was piped");
    let stderr_pipe = child.stderr().take().expect("stderr was piped");

    // Keep output drains independent from timeout selection so timed-out
    // commands can still return buffered stdout/stderr.
    let stdout_task = spawn_pipe_drain_task(stdout_pipe);
    let stderr_task = spawn_pipe_drain_task(stderr_pipe);

    // Race between timeout and process completion. Pipe drain tasks keep running
    // regardless of which branch wins this select.
    let wait_result = tokio::select! {
        biased;  // Check timeout first for consistent behavior

        _ = tokio::time::sleep(timeout) => None,
        status = child.wait() => Some(status),
    };

    match wait_result {
        Some(status) => {
            let (stdout_data, stderr_data) = tokio::join!(
                await_pipe_drain_task(stdout_task),
                await_pipe_drain_task(stderr_task)
            );
            let status = status.map_err(|e| ToolError::Execution(e.to_string()))?;

            Ok(BashOutput {
                exit_code: status.code(),
                stdout: string_from_utf8_or_lossy(stdout_data),
                stderr: string_from_utf8_or_lossy(stderr_data),
            })
        }
        None => {
            // Timeout: explicitly kill the process tree (Job Object on Windows,
            // process group on Unix), then briefly await pipe drains for buffered output.
            let kill_result = Pin::from(child.kill()).await;

            let (stdout_data, stderr_data) = tokio::join!(
                await_pipe_drain_task_with_grace(stdout_task, PIPE_DRAIN_GRACE_PERIOD),
                await_pipe_drain_task_with_grace(stderr_task, PIPE_DRAIN_GRACE_PERIOD)
            );

            Err(timeout_error_with_kill_failure(
                timeout_message_with_buffered_output(timeout, &stdout_data, &stderr_data),
                kill_result.err().map(|e| e.to_string()),
            ))
        }
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
    use crate::permissions::{PermissionAction, Rule, Ruleset};
    use crate::tool_metadata::bash as bash_meta;
    use crate::tools::{BashRequest, BashSettings};
    use tempfile::TempDir;

    #[tokio::test]
    async fn execute_echo_returns_output() {
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
        .await
        .unwrap();

        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn rejected_command_returns_permission_denied() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new(bash_meta::NAME, "*", PermissionAction::Allow));
        ruleset.push(Rule::new(
            bash_meta::NAME,
            "echo hello",
            PermissionAction::Deny,
        ));

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
        .await
        .unwrap_err();

        assert!(matches!(
            err,
            ToolError::PermissionDenied { tool: "bash", .. }
        ));
    }

    #[tokio::test]
    async fn respects_working_directory() {
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
        )
        .await;
        assert!(matches!(
            result,
            Err(ToolError::Timeout(_) | ToolError::TimeoutWithKillFailure { .. })
        ));
    }

    #[tokio::test]
    async fn timeout_preserves_buffered_output() {
        let cmd = if cfg!(target_os = "windows") {
            "echo stdout-before-timeout & echo stderr-before-timeout 1>&2 & ping -n 10 127.0.0.1 >nul"
        } else {
            "echo stdout-before-timeout; echo stderr-before-timeout 1>&2; sleep 10"
        };

        let result = execute_command(
            &BashExecutionMode::Host,
            BashRequest {
                command: cmd.to_string(),
                workdir: None,
                timeout_ms: Some(500),
            },
            BashSettings {
                default_timeout_ms: 5000,
                max_timeout_ms: 10000,
                default_workdir: None,
                permission: None,
            },
        )
        .await;
        match result {
            Err(ToolError::Timeout(message))
            | Err(ToolError::TimeoutWithKillFailure { message, .. }) => {
                assert!(message.contains("stdout-before-timeout"));
                assert!(message.contains("stderr-before-timeout"));
            }
            other => panic!("expected timeout error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn grace_abort_retains_shared_pipe_buffer() {
        use tokio::sync::oneshot;

        let buffer: SharedPipeBuffer = Arc::new(Mutex::new(Vec::with_capacity(32)));
        let task_buffer = Arc::clone(&buffer);
        let (written_tx, written_rx) = oneshot::channel();
        let (_block_tx, block_rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            task_buffer.lock().extend_from_slice(b"partial-output");
            let _ = written_tx.send(());
            let _ = block_rx.await; // block infinitely, task will be cancelled by grace period timeout
        });

        written_rx
            .await
            .expect("drain task should write buffered output before abort");

        let data =
            await_pipe_drain_task_with_grace(PipeDrainTask { handle, buffer }, Duration::ZERO)
                .await;

        assert_eq!(data, b"partial-output");
    }

    #[tokio::test]
    async fn invalid_workdir_returns_error() {
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
        .await
        .unwrap();

        assert_eq!(result.exit_code, Some(0));
        // Verify we got all the output (102400 bytes written)
        assert_eq!(result.stdout.len(), 102400);
    }
}
