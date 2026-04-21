//! Tokio `process-wrap` integration for bubblewrap execution.
//!
//! # Public API
//!
//! - [`build_command_wrap`] — build the async wrapped command

use super::wrap_command;
use crate::{LinuxBwrapError, Profile};
use process_wrap::tokio::{CommandWrap, ProcessGroup};
use std::path::Path;
use std::process::Stdio;

/// Builds an async [`CommandWrap`] from a [`Profile`].
///
/// # Errors
/// - Returns [`LinuxBwrapError::InvalidPath`] when `workdir` is not an absolute path,
///   does not exist, is not a directory, or is not visible inside the sandbox.
pub fn build_command_wrap(
    profile: &Profile,
    command: &str,
    workdir: Option<&Path>,
) -> Result<CommandWrap, LinuxBwrapError> {
    let wrapped = wrap_command(profile, command, workdir)?;

    let mut wrap = CommandWrap::with_new(wrapped.program(), |cmd| {
        cmd.args(wrapped.args());
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    wrap.wrap(ProcessGroup::leader());
    Ok(wrap)
}
