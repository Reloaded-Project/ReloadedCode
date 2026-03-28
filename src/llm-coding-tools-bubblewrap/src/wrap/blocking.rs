//! Blocking `process-wrap` integration for bubblewrap execution.
//!
//! # Public API
//!
//! - [`build_command_wrap`] — build the blocking wrapped command

use super::wrap_command;
use crate::{LinuxBwrapError, Profile};
use process_wrap::std::{CommandWrap, ProcessGroup};
use std::path::Path;
use std::process::Stdio;

/// Builds a sync [`CommandWrap`] from a [`Profile`].
///
/// # Errors
///
/// Returns [`LinuxBwrapError`] on invalid per-command workdir.
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
