//! Builds `bwrap` command lines from a validated sandbox profile.
//!
//! Given a validated [`crate::profile::Profile`], this module appends the
//! per-invocation working directory and shell command to the precomputed static
//! `bwrap` argv prefix and produces a [`LinuxBwrapWrappedCommand`] ready for
//! execution.

use crate::profile::validation::validate_optional_directory_path;
use crate::profile::Profile;
use crate::LinuxBwrapError;
use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// A command wrapped in a `bwrap` sandbox.
///
/// The wrapped command borrows the profile's static data and the caller's shell
/// command string. The working directory is only allocated when it must be
/// rewritten into a different sandbox path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxBwrapWrappedCommand<'a> {
    program: &'a Path,
    static_args: &'a [OsString],
    sandbox_cwd: Cow<'a, Path>,
    shell: &'a Path,
    command: &'a str,
}

impl<'a> LinuxBwrapWrappedCommand<'a> {
    /// Returns the `bwrap` executable path.
    #[inline]
    pub fn program(&self) -> &Path {
        self.program
    }

    /// Returns the number of argv entries that will be passed to `bwrap`.
    #[inline]
    pub fn arg_count(&self) -> usize {
        self.static_args.len() + 6
    }

    /// Returns the complete argv iterator to pass to `bwrap`.
    ///
    /// Pass each element to `std::process::Command::args` (or equivalent).
    /// The tail is always `--chdir <cwd> -- <shell> -c <command>`.
    #[inline]
    pub fn args(&self) -> impl Iterator<Item = &OsStr> + Clone + '_ {
        self.static_args
            .iter()
            .map(OsString::as_os_str)
            .chain(Some(OsStr::new("--chdir")))
            .chain(Some(self.sandbox_cwd.as_os_str()))
            .chain(Some(OsStr::new("--")))
            .chain(Some(self.shell.as_os_str()))
            .chain(Some(OsStr::new("-c")))
            .chain(Some(OsStr::new(self.command)))
    }
}

#[inline]
fn resolve_sandbox_cwd<'a>(
    profile: &'a Profile,
    workdir: Option<&'a Path>,
) -> Result<Cow<'a, Path>, LinuxBwrapError> {
    if let Some(dir) = workdir {
        if !profile.is_prevalidated_workdir(dir) {
            validate_workdir(Some(dir))?;
        }
    }
    profile.map_workdir_to_sandbox(workdir)
}

/// Builds a `bwrap` command line that runs `command` inside the sandbox
/// described by `profile`.
///
/// # Errors
/// - Returns [`LinuxBwrapError::InvalidPath`] when `workdir` is not an absolute path.
/// - Returns [`LinuxBwrapError::InvalidPath`] when `workdir` does not exist or is not a directory.
/// - Returns [`LinuxBwrapError::InvalidPath`] when `workdir` is not visible inside the sandbox
///   (not under workspace, synthetic home, cache root, or any mounted directory).
#[inline]
pub fn wrap_command<'a>(
    profile: &'a Profile,
    command: &'a str,
    workdir: Option<&'a Path>,
) -> Result<LinuxBwrapWrappedCommand<'a>, LinuxBwrapError> {
    Ok(LinuxBwrapWrappedCommand {
        program: profile.bwrap_program(),
        static_args: profile.static_args(),
        sandbox_cwd: resolve_sandbox_cwd(profile, workdir)?,
        shell: profile.shell(),
        command,
    })
}

/// Rejects non-absolute or non-existent working directories.
fn validate_workdir(workdir: Option<&Path>) -> Result<(), LinuxBwrapError> {
    validate_optional_directory_path(workdir, "working directory")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{args_as_strings, write_script, SandboxFixture};
    use crate::{Availability, NetworkPolicy, Preset};
    use serial_test::serial;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    #[serial]
    fn build_command_line_orders_env_mounts_and_cwd() {
        let fixture = SandboxFixture::new("exit 0");
        let ro_mount = fixture.make_dir("ro");
        let rw_mount = fixture.make_dir("rw");

        let default_env: Arc<[crate::EnvVar]> =
            Arc::new([crate::EnvVar::new("HOME", "/home/user")]);
        let extra_env: Arc<[crate::EnvVar]> = Arc::new([crate::EnvVar::new("FOO", "bar")]);
        let ro_mounts: Arc<[Box<Path>]> = Arc::new([
            ro_mount.clone().into_boxed_path(),
            fixture.temp_path().to_path_buf().into_boxed_path(),
        ]);
        let rw_mounts: Arc<[Box<Path>]> = Arc::new([rw_mount.clone().into_boxed_path()]);

        let profile = crate::Builder::new(
            fixture.workspace(),
            fixture.home(),
            fixture.cache(),
            crate::TmpBacking::Tmpfs,
        )
        .with_preset(Preset::PublicBot)
        .with_clear_env(true)
        .with_default_env(default_env)
        .with_extra_env(extra_env)
        .with_read_only_mounts(ro_mounts)
        .with_read_write_mounts(rw_mounts)
        .with_network_policy(NetworkPolicy::Disabled)
        .with_availability(Availability::Available)
        .build()
        .unwrap();

        let cmd_line = wrap_command(&profile, "echo hello", None).unwrap();
        let args = args_as_strings(cmd_line.args());

        let clearenv_pos = args.iter().position(|a| a == "--clearenv").unwrap();
        let setenv_home_pos = args.iter().position(|a| a == "--setenv").unwrap();
        assert!(clearenv_pos < setenv_home_pos);
        assert!(args.contains(&"--unshare-net".to_string()));
        let ro_bind_pos = args.iter().position(|a| a == "--ro-bind").unwrap();
        let bind_positions: Vec<_> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--bind")
            .map(|(i, _)| i)
            .collect();
        for bind_pos in &bind_positions {
            assert!(ro_bind_pos < *bind_pos);
        }

        let chdir_pos = args.iter().position(|a| a == "--chdir").unwrap();
        assert_eq!(args[chdir_pos + 1], fixture.workspace().to_string_lossy());
        assert!(args.contains(&"--proc".to_string()));
        assert!(args.contains(&"--dev".to_string()));
        assert!(args.contains(&"--tmpfs".to_string()));
    }

    #[test]
    #[serial]
    fn build_command_line_uses_workdir_over_workspace() {
        let fixture = SandboxFixture::new("exit 0");
        let workdir = fixture.workspace().join("subdir");
        fs::create_dir_all(&workdir).unwrap();
        let profile = fixture.public_bot_profile().unwrap();

        let cmd_line = wrap_command(&profile, "echo hello", Some(&workdir)).unwrap();
        let args = args_as_strings(cmd_line.args());

        let chdir_pos = args.iter().position(|a| a == "--chdir").unwrap();
        assert_eq!(args[chdir_pos + 1], "/workspace/subdir");
    }

    #[test]
    #[serial]
    fn public_bot_defaults_emit_expected_bwrap_argv() {
        let fixture = SandboxFixture::new("exit 0");
        let profile = fixture.public_bot_profile().unwrap();

        let cmd_line = wrap_command(&profile, "echo hello", None).unwrap();
        let args = args_as_strings(cmd_line.args());

        assert!(args.contains(&"--clearenv".to_string()));
        assert!(args.iter().any(|a| a == "--setenv"));
        assert!(args.contains(&"PATH".to_string()));
        let path_pos = args.iter().position(|a| a == "PATH").unwrap();
        assert!(
            args[path_pos + 1].contains("/usr/bin")
                || args[path_pos + 1].contains("/run/current-system/sw/bin")
                || args[path_pos + 1].contains("/nix/var/nix/profiles/default/bin")
        );
        assert!(args.contains(&"HOME".to_string()));
        assert!(args.contains(&"/home/sandbox".to_string()));
        assert!(args.contains(&"--unshare-net".to_string()));
        assert!(
            args.contains(&"/usr/bin".to_string())
                || args.contains(&"/run/current-system/sw".to_string())
                || args.contains(&"/nix/store".to_string())
        );
        assert!(args.contains(&"--dev".to_string()));
        assert!(args.contains(&"--proc".to_string()));
        assert!(args.contains(&"--tmpfs".to_string()));
        let bind_positions: Vec<_> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--bind")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(bind_positions.len(), 2);
        assert!(!args.contains(&fixture.cache().to_string_lossy().to_string()));
        let dash_pos = args.iter().position(|a| a == "--").unwrap();
        assert!(Path::new(&args[dash_pos + 1]).is_absolute());
        assert!(args[dash_pos + 1].ends_with("/bash") || args[dash_pos + 1].ends_with("/sh"));
        assert_eq!(args[dash_pos + 2], "-c");
        assert_eq!(args[dash_pos + 3], "echo hello");
    }

    #[test]
    #[serial]
    fn trusted_maintenance_allows_visible_absolute_workdir() {
        let fixture = SandboxFixture::new("exit 0");
        let host_tmp = fixture.make_dir("host-tmp");
        let outside = ["/usr/bin", "/usr", "/bin", "/etc", "/var"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.is_dir())
            .expect("expected a visible host directory outside /home and /tmp");

        let profile = fixture.trusted_maintenance_profile(&host_tmp).unwrap();

        let cmd_line = wrap_command(&profile, "pwd", Some(outside)).unwrap();
        let args = args_as_strings(cmd_line.args());
        let chdir_pos = args.iter().position(|arg| arg == "--chdir").unwrap();
        assert_eq!(args[chdir_pos + 1], outside.to_string_lossy());
    }

    #[test]
    #[serial]
    fn explicit_mount_workdir_maps_to_same_path_mount() {
        let fixture = SandboxFixture::new("exit 0");
        let ro_mount = fixture.make_dir("shared");
        let nested = ro_mount.join("subdir");

        fs::create_dir_all(&nested).unwrap();

        let profile = crate::Builder::new(
            fixture.workspace(),
            fixture.home(),
            fixture.cache(),
            crate::TmpBacking::Tmpfs,
        )
        .with_read_only_mounts(Arc::from([
            fixture.temp_path().to_path_buf().into_boxed_path(),
            ro_mount.clone().into_boxed_path(),
        ]))
        .build()
        .unwrap();

        let cmd_line = wrap_command(&profile, "pwd", Some(nested.as_path())).unwrap();
        let args = args_as_strings(cmd_line.args());
        let chdir_pos = args.iter().position(|arg| arg == "--chdir").unwrap();
        assert_eq!(args[chdir_pos + 1], nested.to_string_lossy());
    }

    #[test]
    #[serial]
    fn path_changes_after_build_do_not_change_prevalidated_shell() {
        let fixture = SandboxFixture::new("exit 0");
        let fake_bin = fixture.make_dir("fake-bin");
        let profile = fixture.public_bot_profile().unwrap();

        write_script(&fake_bin, "bash", "#!/bin/sh\nexit 0\n");
        unsafe { env::set_var("PATH", &fake_bin) };

        let cmd_line = wrap_command(&profile, "echo hello", None).unwrap();
        let args = args_as_strings(cmd_line.args());
        let dash_pos = args.iter().position(|arg| arg == "--").unwrap();
        assert_eq!(
            PathBuf::from(&args[dash_pos + 1]),
            profile.shell().to_path_buf()
        );
    }
}
