//! Shared test helpers for bubblewrap unit tests.
//!
//! Provides environment isolation ([`PathGuard`], `replace_path`, `prepend_path`),
//! fake binary scaffolding (`create_fake_bwrap`, `create_fake_shell`), and
//! reusable sandbox fixtures ([`SandboxDirs`], [`SandboxFixture`]) that set up
//! temp directory layouts and managed `PATH` overrides.

use crate::probe::PROBE_ARG0;
use crate::{LinuxBwrapError, Profile, TmpBacking};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const DEFAULT_FAKE_SHELL: &str = "#!/bin/sh\nexit 0\n";

/// Captures the original `PATH` and restores it on drop.
///
/// Used alongside [`replace_path`] or [`prepend_path`] so that test
/// environment changes are automatically cleaned up.
pub(crate) struct PathGuard(Option<OsString>);

impl PathGuard {
    /// Snapshots the current `PATH` value (or notes that it is unset).
    pub(crate) fn capture() -> Self {
        Self(env::var_os("PATH"))
    }
}

impl Drop for PathGuard {
    /// Restores the `PATH` that was active when this guard was created.
    ///
    /// Uses `unsafe` `env::set_var` / `env::remove_var` because Rust's safe
    /// API does not permit modifying environment variables during program
    /// execution. This is safe in test-only code where single-threaded
    /// environment access is guaranteed.
    fn drop(&mut self) {
        match &self.0 {
            Some(path) => unsafe { env::set_var("PATH", path) },
            None => unsafe { env::remove_var("PATH") },
        }
    }
}

/// Replaces `PATH` with `path` and returns a [`PathGuard`] that restores it.
///
/// # Safety
/// The returned guard restores `PATH` via `unsafe` env-var APIs on drop.
/// Callers must ensure no other thread reads `PATH` concurrently.
pub(crate) fn replace_path(path: &Path) -> PathGuard {
    let guard = PathGuard::capture();
    unsafe { env::set_var("PATH", path) };
    guard
}

/// Prepends `path` to `PATH` and returns a [`PathGuard`] that restores it.
///
/// If the current `PATH` is empty or unset, the result is just `path`.
///
/// # Safety
/// The returned guard restores `PATH` via `unsafe` env-var APIs on drop.
/// Callers must ensure no other thread reads `PATH` concurrently.
pub(crate) fn prepend_path(path: &Path) -> PathGuard {
    let guard = PathGuard::capture();
    let prefix = path.to_string_lossy();
    let original = guard.0.as_ref().map(|value| value.to_string_lossy());
    let capacity = prefix.len() + original.as_ref().map_or(0, |value| value.len() + 1);
    let mut new_path = String::with_capacity(capacity);
    new_path.push_str(&prefix);
    if let Some(original) = original {
        if !original.is_empty() {
            new_path.push(':');
            new_path.push_str(&original);
        }
    }
    unsafe { env::set_var("PATH", &new_path) };
    guard
}

/// Writes `contents` to `path` and marks it executable on Unix.
///
/// # Panics
/// Propagates any I/O error from the write or permission change.
pub(crate) fn write_executable(path: &Path, contents: impl AsRef<[u8]>) {
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}

/// Writes an executable script inside `dir` and returns its path.
///
/// # Panics
/// Propagates any I/O error.
pub(crate) fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    write_executable(&path, body.as_bytes());
    path
}

/// Creates a fake `bash` binary that exits successfully.
///
/// The script is a minimal `/bin/sh` wrapper that returns exit code 0.
pub(crate) fn create_fake_shell(dir: &Path) -> PathBuf {
    write_script(dir, "bash", DEFAULT_FAKE_SHELL)
}

/// Creates a fake `bwrap` script in `dir`.
///
/// Returns the log file path. The fake binary handles `--version` and the
/// probe command itself, logs other arguments to `bwrap.log`, and then runs
/// `behavior`.
pub(crate) fn create_fake_bwrap(dir: &Path, behavior: &str) -> PathBuf {
    let bwrap_path = dir.join("bwrap");
    let log_path = dir.join("bwrap.log");
    let log_path_escaped = log_path.to_string_lossy().replace('\'', "'\\''");
    let script = format!(
        r#"#!/bin/sh
# Handle --version probe
for arg in "$@"; do
    if [ "$arg" = "--version" ]; then
        echo "bubblewrap 0.8.0"
        exit 0
    fi
done
# Handle capability probe via the unique shell command marker.
for arg in "$@"; do
    case "$arg" in
        {probe_arg0})
            exit 0
            ;;
    esac
done
# Log arguments for verification
for a in "$@"; do
    printf '%s\n' "$a" >> '{log_path_escaped}'
done
echo "" >> '{log_path_escaped}'
# Execute the provided behavior
{behavior}
"#,
        behavior = behavior,
        log_path_escaped = log_path_escaped,
        probe_arg0 = PROBE_ARG0,
    );
    write_executable(&bwrap_path, script.as_bytes());
    log_path
}

/// Standard sandbox directory layout used across tests.
///
/// Owns a [`TempDir`] containing `workspace`, `home`, and `cache`
/// subdirectories. Dropping this value removes the entire tree.
pub(crate) struct SandboxDirs {
    temp: TempDir,
    workspace: PathBuf,
    home: PathBuf,
    cache: PathBuf,
}

impl SandboxDirs {
    /// Creates a tempdir with `workspace`, `home`, and `cache` subdirectories.
    ///
    /// # Panics
    /// Propagates any I/O error from tempdir creation or `create_dir_all`.
    pub(crate) fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let workspace = temp.path().join("workspace");
        let home = temp.path().join("home");
        let cache = temp.path().join("cache");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&home).unwrap();
        fs::create_dir(&cache).unwrap();
        Self {
            temp,
            workspace,
            home,
            cache,
        }
    }

    /// Returns the temp root path.
    pub(crate) fn temp_path(&self) -> &Path {
        self.temp.path()
    }

    /// Returns the workspace path.
    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Returns the home path.
    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    /// Returns the cache path.
    pub(crate) fn cache(&self) -> &Path {
        &self.cache
    }

    /// Creates a named directory inside the temp root.
    ///
    /// # Panics
    /// Propagates any I/O error.
    pub(crate) fn make_dir(&self, name: &str) -> PathBuf {
        let path = self.temp_path().join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }
}

/// Shared sandbox fixture with fake binaries and a managed `PATH`.
pub(crate) struct SandboxFixture {
    dirs: SandboxDirs,
    _path_guard: PathGuard,
}

impl SandboxFixture {
    /// Creates a fixture whose temp root fully replaces `PATH`.
    pub(crate) fn new(bwrap_behavior: &str) -> Self {
        Self::with_path_mode(bwrap_behavior, false)
    }

    /// Creates a fixture whose temp root is prepended to `PATH`.
    #[allow(dead_code)]
    pub(crate) fn with_prepended_path(bwrap_behavior: &str) -> Self {
        Self::with_path_mode(bwrap_behavior, true)
    }

    fn with_path_mode(bwrap_behavior: &str, prepend: bool) -> Self {
        let dirs = SandboxDirs::new();
        create_fake_bwrap(dirs.temp_path(), bwrap_behavior);
        create_fake_shell(dirs.temp_path());
        let _path_guard = if prepend {
            prepend_path(dirs.temp_path())
        } else {
            replace_path(dirs.temp_path())
        };
        Self { dirs, _path_guard }
    }

    /// Returns the temp root path.
    pub(crate) fn temp_path(&self) -> &Path {
        self.dirs.temp_path()
    }

    /// Returns the workspace path.
    pub(crate) fn workspace(&self) -> &Path {
        self.dirs.workspace()
    }

    /// Returns the home path.
    pub(crate) fn home(&self) -> &Path {
        self.dirs.home()
    }

    /// Returns the cache path.
    pub(crate) fn cache(&self) -> &Path {
        self.dirs.cache()
    }

    /// Creates a named directory inside the temp root.
    pub(crate) fn make_dir(&self, name: &str) -> PathBuf {
        self.dirs.make_dir(name)
    }

    /// Overwrites the fake `bash` binary with a custom script.
    #[allow(dead_code)]
    pub(crate) fn write_shell(&self, body: &str) -> PathBuf {
        write_script(self.temp_path(), "bash", body)
    }

    /// Overwrites the fake `bwrap` binary with a custom behavior script.
    #[allow(dead_code)]
    pub(crate) fn write_bwrap(&self, behavior: &str) -> PathBuf {
        create_fake_bwrap(self.temp_path(), behavior)
    }

    /// Builds the standard public-bot test profile.
    ///
    /// # Returns
    /// `Ok([`Profile`])` on success, or `Err([`LinuxBwrapError`])` if
    /// profile construction fails.
    pub(crate) fn public_bot_profile(&self) -> Result<Profile, LinuxBwrapError> {
        Profile::public_bot_defaults(
            self.workspace(),
            self.home(),
            self.cache(),
            Some(TmpBacking::Tmpfs),
        )
    }

    /// Builds the standard trusted-maintenance test profile.
    ///
    /// # Returns
    /// `Ok([`Profile`])` on success, or `Err([`LinuxBwrapError`])` if
    /// profile construction fails.
    pub(crate) fn trusted_maintenance_profile(
        &self,
        host_tmp: &Path,
    ) -> Result<Profile, LinuxBwrapError> {
        Profile::trusted_maintenance_defaults(self.workspace(), self.home(), self.cache(), host_tmp)
    }
}

/// Converts command args into owned strings for assertions.
pub(crate) fn args_as_strings<'a>(
    args: impl IntoIterator<Item = &'a std::ffi::OsStr>,
) -> Vec<String> {
    args.into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}
