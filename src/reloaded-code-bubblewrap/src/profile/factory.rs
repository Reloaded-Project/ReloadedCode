//! Convenience constructors for sandbox profiles.
//!
//! Use [`create_sandbox`] for preset profiles or [`create_sandbox_with`] for
//! custom builders, passing a [`SandboxDirs`] that specifies the host
//! directory paths. Use [`create_temp_sandbox`] for the common case where
//! all directories are auto-managed temporaries.
//!
//! # Directory ownership
//!
//! [`SandboxDirs`] borrows its paths, so the backing storage must outlive
//! the `create_sandbox` / `create_sandbox_with` call. When all directories
//! are ephemeral, [`TempSandboxDirs`] owns the temp tree and
//! [`TempSandboxDirs::as_dirs`] provides the borrowed view.

use super::builder::Builder;
use super::types::{Availability, Preset, Profile};
use crate::LinuxBwrapError;
use std::path::Path;
use std::sync::Arc;

/// Borrowed directory paths for sandbox construction.
///
/// Lightweight view over three host directories that a sandbox profile
/// needs: a synthetic home, a cache root, and a host-tmp directory.
/// Because the fields are borrowed references, the backing storage must
/// outlive this value.
///
/// Use [`TempSandboxDirs::as_dirs`] when all directories come from an
/// auto-managed temp tree, or construct this directly when mixing
/// persisted and ephemeral directories.
///
/// # Examples
///
/// Mixed persistent cache with ephemeral home and host-tmp:
///
/// ```no_run
/// use reloaded_code_bubblewrap::profile::SandboxDirs;
/// use std::path::Path;
///
/// let temp = reloaded_code_bubblewrap::TempSandboxDirs::new().unwrap();
/// let dirs = SandboxDirs::new(
///     temp.home(),                    // ephemeral
///     Path::new("/persistent/cache"), // survives across sessions
///     temp.host_tmp(),               // ephemeral
/// );
/// ```
pub struct SandboxDirs<'a> {
    home: &'a Path,
    cache: &'a Path,
    host_tmp: &'a Path,
}

impl<'a> SandboxDirs<'a> {
    /// Creates a new directory spec from borrowed host paths.
    ///
    /// # Arguments
    /// - `home` - Host path to the synthetic home directory.
    /// - `cache` - Host path to the cache root directory.
    /// - `host_tmp` - Host path to the host-tmp directory.
    pub fn new(home: &'a Path, cache: &'a Path, host_tmp: &'a Path) -> Self {
        Self {
            home,
            cache,
            host_tmp,
        }
    }

    /// Returns the host path to the synthetic home directory.
    pub fn home(&self) -> &'a Path {
        self.home
    }

    /// Returns the host path to the cache root directory.
    pub fn cache(&self) -> &'a Path {
        self.cache
    }

    /// Returns the host path to the host-tmp directory.
    pub fn host_tmp(&self) -> &'a Path {
        self.host_tmp
    }
}

/// Auto-managed temp directory layout for sandbox construction.
///
/// Creates a temp directory with `home`, `cache`, and `host-tmp`
/// subdirectories. The `cache` subdirectory also gets `xdg-cache` and
/// `xdg-state` sub-subdirectories created by [`Builder::build`].
///
/// Wrapped in [`Arc`] when returned from [`create_temp_sandbox`] so it can
/// be stored alongside the profile in shared state.
pub struct TempSandboxDirs {
    tmpdir: tempfile::TempDir,
    home: Box<Path>,
    cache: Box<Path>,
    host_tmp: Box<Path>,
}

impl TempSandboxDirs {
    /// Creates a new temp directory layout.
    ///
    /// # Returns
    /// - `Ok(Self)`: A temp directory layout with `home`, `cache`, and
    ///   `host-tmp` subdirectories created under a system temp directory.
    ///
    /// # Errors
    /// - Returns [`std::io::Error`] when the system temp directory cannot be
    ///   created (e.g., no writable temp dir, disk full, or permission denied),
    ///   or when any subdirectory (`home`, `cache`, or `host-tmp`) cannot be
    ///   created inside the temp directory.
    pub fn new() -> std::io::Result<Self> {
        let tmpdir = tempfile::Builder::new()
            .prefix("reloaded-code-sandbox-")
            .tempdir()?;

        let home = tmpdir.path().join("home").into_boxed_path();
        let cache = tmpdir.path().join("cache").into_boxed_path();
        let host_tmp = tmpdir.path().join("host-tmp").into_boxed_path();

        // Builder::build() validates directories exist, so create them first.
        std::fs::create_dir_all(&home)?;
        std::fs::create_dir_all(&cache)?;
        std::fs::create_dir_all(&host_tmp)?;

        Ok(Self {
            tmpdir,
            home,
            cache,
            host_tmp,
        })
    }

    /// Returns a [`SandboxDirs`] view over this temp directory layout.
    ///
    /// Useful for passing to [`create_sandbox`] or [`create_sandbox_with`]
    /// when all directories come from this auto-managed temp tree.
    pub fn as_dirs(&self) -> SandboxDirs<'_> {
        SandboxDirs::new(self.home(), self.cache(), self.host_tmp())
    }

    /// Returns the host path to the synthetic home directory.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Returns the host path to the cache root directory.
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// Returns the host path to the host-tmp directory.
    pub fn host_tmp(&self) -> &Path {
        &self.host_tmp
    }

    /// Returns a reference to the underlying temp directory.
    pub fn temp_dir(&self) -> &tempfile::TempDir {
        &self.tmpdir
    }
}

/// Errors that can occur while creating a sandbox profile.
#[derive(Debug, thiserror::Error)]
pub enum CreateSandboxError {
    /// Failed to create the sandbox directory layout.
    #[error("failed to create sandbox directories: {0}")]
    Dirs(#[source] std::io::Error),
    /// Bubblewrap is not available on the host.
    #[error("bubblewrap is not available: {0}")]
    Unavailable(String),
    /// Profile validation or assembly failed.
    #[error("profile validation failed: {0}")]
    Profile(#[from] LinuxBwrapError),
}

/// Creates a sandbox from a preset and a directory spec.
///
/// # Arguments
/// - `workspace`: Host path to the project directory mounted inside the sandbox.
/// - `preset`: The sandbox preset controlling mount layout and permissions.
/// - `dirs`: The host directory paths for home, cache, and host-tmp.
///
/// # Returns
/// - `Ok(`[`Arc<Profile>`]`)`: A ready-to-use sandbox profile.
///
/// # Errors
/// - Returns [`CreateSandboxError::Unavailable`] when `bwrap` is not found on
///   `PATH` or is otherwise unusable on the host (see [`Availability::detect`]).
/// - Returns [`CreateSandboxError::Profile`] when [`Builder::build`] returns
///   [`LinuxBwrapError`] (e.g., invalid paths, missing host shell inside the
///   sandbox).
pub fn create_sandbox(
    workspace: &Path,
    preset: Preset,
    dirs: &SandboxDirs<'_>,
) -> Result<Arc<Profile>, CreateSandboxError> {
    let availability = detect_availability()?;
    // Select the builder preset for the desired sandbox policy.
    let builder = match preset {
        Preset::TrustedMaintenance => {
            Builder::trusted_maintenance(workspace, dirs.home(), dirs.cache(), dirs.host_tmp())
        }
        Preset::PublicBot => Builder::public_bot(workspace, dirs.home(), dirs.cache(), None),
    };
    create_sandbox_inner(builder, availability)
}

/// Creates a sandbox with a custom builder and a directory spec.
///
/// Availability detection is handled automatically.
///
/// # Arguments
/// - `workspace`: Host path to the project directory mounted inside the sandbox.
/// - `dirs`: The host directory paths for home, cache, and host-tmp.
/// - `f`: Closure receiving the workspace path and a [`SandboxDirs`] view;
///   must return a configured [`Builder`].
///
/// # Returns
/// - `Ok(`[`Arc<Profile>`]`)`: A ready-to-use sandbox profile.
///
/// # Errors
/// - Returns [`CreateSandboxError::Unavailable`] when `bwrap` is not found on
///   `PATH` or is otherwise unusable on the host (see [`Availability::detect`]).
/// - Returns [`CreateSandboxError::Profile`] when [`Builder::build`] returns
///   [`LinuxBwrapError`] (e.g., invalid paths, missing host shell inside the
///   sandbox).
pub fn create_sandbox_with<F>(
    workspace: &Path,
    dirs: &SandboxDirs<'_>,
    f: F,
) -> Result<Arc<Profile>, CreateSandboxError>
where
    F: FnOnce(&Path, &SandboxDirs<'_>) -> Builder,
{
    let availability = detect_availability()?;
    let builder = f(workspace, dirs);
    create_sandbox_inner(builder, availability)
}

/// Creates a sandbox from a preset with auto-managed temp directories.
///
/// Convenience wrapper that creates a [`TempSandboxDirs`] and builds a
/// sandbox profile. The temp directory is wrapped in [`Arc`] so it can be
/// stored alongside the profile in shared state.
///
/// # Directory layout
///
/// See [`TempSandboxDirs`].
///
/// # Arguments
/// - `workspace`: Host path to the project directory mounted inside the sandbox.
/// - `preset`: The sandbox preset controlling mount layout and permissions.
///
/// # Returns
/// - `Ok((`[`Arc<Profile>`]`, `[`Arc<TempSandboxDirs>`]`))`: A ready-to-use
///   sandbox profile and its owning temp directory layout.
///
/// # Errors
/// - Returns [`CreateSandboxError::Dirs`] when the system temp directory or
///   any subdirectory cannot be created (see [`TempSandboxDirs::new`]).
/// - Returns [`CreateSandboxError::Unavailable`] when `bwrap` is not found on
///   `PATH` or is otherwise unusable on the host (see [`Availability::detect`]).
/// - Returns [`CreateSandboxError::Profile`] when [`Builder::build`] returns
///   [`LinuxBwrapError`] (e.g., invalid paths, missing host shell inside the
///   sandbox).
pub fn create_temp_sandbox(
    workspace: &Path,
    preset: Preset,
) -> Result<(Arc<Profile>, Arc<TempSandboxDirs>), CreateSandboxError> {
    let availability = detect_availability()?;
    let dirs = TempSandboxDirs::new().map_err(CreateSandboxError::Dirs)?;
    // Select the builder preset for the desired sandbox policy.
    let builder = match preset {
        Preset::TrustedMaintenance => {
            Builder::trusted_maintenance(workspace, dirs.home(), dirs.cache(), dirs.host_tmp())
        }
        Preset::PublicBot => Builder::public_bot(workspace, dirs.home(), dirs.cache(), None),
    };
    let profile = create_sandbox_inner(builder, availability)?;
    Ok((profile, Arc::new(dirs)))
}

// Check if bwrap is available on the host.
fn detect_availability() -> Result<Availability, CreateSandboxError> {
    let availability = Availability::detect();
    if !availability.is_available() {
        return Err(CreateSandboxError::Unavailable(
            availability
                .reason()
                .unwrap_or("unknown reason")
                .to_string(),
        ));
    }
    Ok(availability)
}

fn create_sandbox_inner(
    builder: Builder,
    availability: Availability,
) -> Result<Arc<Profile>, CreateSandboxError> {
    let profile = builder
        .with_availability(availability)
        .build()
        .map_err(CreateSandboxError::Profile)?;
    Ok(Arc::new(profile))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_sandbox_dirs_creates_layout_and_as_dirs_returns_matching_view() {
        let dirs = TempSandboxDirs::new().unwrap();

        assert!(dirs.home().is_dir());
        assert!(dirs.cache().is_dir());
        assert!(dirs.host_tmp().is_dir());

        assert!(dirs.home().ends_with("home"));
        assert!(dirs.cache().ends_with("cache"));
        assert!(dirs.host_tmp().ends_with("host-tmp"));

        let tmpdir_prefix = dirs
            .temp_dir()
            .path()
            .file_name()
            .expect("temp dir has a name")
            .to_string_lossy();
        assert!(
            tmpdir_prefix.starts_with("reloaded-code-sandbox-"),
            "unexpected prefix: {tmpdir_prefix}",
        );

        let view = dirs.as_dirs();
        assert_eq!(view.home(), dirs.home());
        assert_eq!(view.cache(), dirs.cache());
        assert_eq!(view.host_tmp(), dirs.host_tmp());
    }
}
