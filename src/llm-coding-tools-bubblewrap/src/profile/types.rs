//! Types for bubblewrap profiles and related settings.
//!
//! Main types:
//! - [`crate::profile::Profile`] - validated sandbox profile
//! - [`crate::profile::Preset`] - preset name stored on the profile
//! - [`crate::profile::TmpBacking`] - how sandbox `/tmp` is mounted
//! - [`crate::profile::Availability`] - whether `bwrap` can run

use super::layout::{join_mapped_path, PathMapping, SandboxLayout};
use crate::LinuxBwrapError;
use std::borrow::Cow;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

/// Preset names for common sandbox setups.
///
/// [`Self::TrustedMaintenance`] is only for trusted jobs. It keeps network
/// access enabled, so a command can send out any data it can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Safer defaults for untrusted or public input.
    ///
    /// This preset mounts selected system paths, the workspace, the synthetic
    /// home, `/dev`, `/proc`, and `/tmp`. It does not expose the real home
    /// directory or inherited env vars.
    PublicBot,
    /// Broader defaults for trusted jobs.
    ///
    /// This preset keeps network access enabled and exposes the host root
    /// read-only. Do not use it for untrusted input.
    TrustedMaintenance,
}

/// Network policy for Linux sandbox execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkPolicy {
    /// Network access is disabled (default).
    #[default]
    Disabled,
    /// Network access is enabled.
    Enabled,
}

/// How sandbox `/tmp` is mounted.
///
/// Use [`Self::Tmpfs`] to keep `/tmp` in memory. Use [`Self::BindHost`] to
/// mount a host directory at `/tmp`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TmpBacking {
    /// Mount `/tmp` as tmpfs inside the sandbox.
    #[default]
    Tmpfs,
    /// Mount a host directory at sandbox `/tmp`.
    ///
    /// You create and clean up the directory.
    BindHost(Box<Path>),
}

/// Whether bubblewrap can run.
///
/// Stores the check result and, when unavailable, the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// Availability has not been checked yet.
    Unknown,
    /// Bubblewrap is available.
    Available,
    /// Bubblewrap cannot run.
    Unavailable {
        /// Why bubblewrap is unavailable.
        reason: Box<str>,
    },
}

impl Availability {
    /// Checks whether bubblewrap can run in the current process.
    ///
    /// # Returns
    /// - [`Availability::Available`] when `bwrap` is present and usable.
    /// - [`Availability::Unavailable`] with an actionable reason otherwise.
    pub fn detect() -> Self {
        crate::probe::probe_availability()
    }

    /// Creates an unavailable state with a reason.
    ///
    /// # Examples
    /// ```
    /// use llm_coding_tools_bubblewrap::profile::Availability;
    ///
    /// let avail = Availability::unavailable("bwrap not found");
    /// assert!(!avail.is_available());
    /// ```
    pub fn unavailable(reason: impl Into<Box<str>>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    /// Returns the reason when bubblewrap is unavailable.
    ///
    /// Returns `None` for `Unknown` and `Available`.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable { reason } => Some(reason.as_ref()),
            Self::Unknown | Self::Available => None,
        }
    }

    /// Returns whether bubblewrap is known to be available.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// One environment variable for the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    name: Box<str>,
    value: Box<str>,
}

impl EnvVar {
    /// Creates an environment variable.
    ///
    /// # Arguments
    /// - `name` - The variable name, such as `PATH` or `HOME`.
    /// - `value` - The variable value.
    pub fn new(name: impl Into<Box<str>>, value: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }

    /// Returns the variable name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the variable value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// One symlink to create inside the sandbox root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symlink {
    target: Box<str>,
    link_path: Box<Path>,
}

impl Symlink {
    /// Creates a symlink entry.
    ///
    /// # Arguments
    /// - `target` - The symlink target path.
    /// - `link_path` - The path where the symlink is created inside the sandbox.
    pub fn new(target: impl Into<Box<str>>, link_path: impl Into<Box<Path>>) -> Self {
        Self {
            target: target.into(),
            link_path: link_path.into(),
        }
    }

    /// Returns the symlink target.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the link path inside the sandbox.
    pub fn link_path(&self) -> &Path {
        &self.link_path
    }
}

/// One read-only file mount inside the sandbox.
///
/// # Validation
/// - The source must be an absolute regular file on the host.
/// - The destination must stay under the mounted synthetic home, workspace, or cache root.
/// - Directory mounts, sockets, and agent forwarding are not allowed.
///
/// Make sure the destination parent directory exists before launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMount {
    source: Box<Path>,
    dest: Box<Path>,
}

impl FileMount {
    /// Creates a file mount.
    ///
    /// # Arguments
    /// - `source` - The source file path on the host.
    /// - `dest` - The destination path inside the sandbox.
    pub fn new(source: impl Into<Box<Path>>, dest: impl Into<Box<Path>>) -> Self {
        Self {
            source: source.into(),
            dest: dest.into(),
        }
    }

    /// Returns the source file path on the host.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Returns the destination path inside the sandbox.
    pub fn dest(&self) -> &Path {
        &self.dest
    }
}

/// One read-only file overlay inside the sandbox.
///
/// Replaces a file anywhere in the sandbox rootfs with content from a host
/// file via a read-only bind-mount. Unlike [`FileMount`], the destination is
/// not restricted to mounted prefixes - it can target any absolute path such
/// as `/etc/shadow` or `/etc/hostname`.
///
/// # Validation
/// - The source must be an absolute path that exists on the host.
/// - The destination must be an absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOverlay {
    source: Box<Path>,
    dest: Box<Path>,
}

impl FileOverlay {
    /// Creates a file overlay.
    ///
    /// # Arguments
    /// - `source` - The host file whose content is bind-mounted read-only.
    /// - `dest` - The sandbox path to be replaced.
    pub fn new(source: impl Into<Box<Path>>, dest: impl Into<Box<Path>>) -> Self {
        Self {
            source: source.into(),
            dest: dest.into(),
        }
    }

    /// Returns the host source file path.
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// Returns the sandbox destination path.
    pub fn dest(&self) -> &Path {
        &self.dest
    }
}

/// A validated bubblewrap profile ready for repeated command wrapping.
///
/// Build this with [`crate::profile::Builder::build`](crate::profile::Builder::build).
///
/// The build step validates profile-owned paths, resolves the `bwrap` binary,
/// picks a visible host shell, and precomputes the static `bwrap` argv prefix.
/// [`crate::wrap::wrap_command`] only needs to map the per-call working
/// directory and append the shell command tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub(crate) preset: Option<Preset>,
    pub(crate) workspace: Box<Path>,
    pub(crate) workspace_dest: Box<Path>,
    pub(crate) synthetic_home: Box<Path>,
    pub(crate) synthetic_home_dest: Box<Path>,
    pub(crate) cache_root: Box<Path>,
    pub(crate) tmp_backing: TmpBacking,
    pub(crate) mount_cache_root: bool,
    pub(crate) compat_symlinks: Arc<[Symlink]>,
    pub(crate) read_only_mounts: Arc<[Box<Path>]>,
    pub(crate) read_write_mounts: Arc<[Box<Path>]>,
    pub(crate) tmpfs_overlays: Arc<[Box<Path>]>,
    pub(crate) file_overlays: Arc<[FileOverlay]>,
    pub(crate) credential_file_mounts: Arc<[FileMount]>,
    pub(crate) read_only_host_rootfs: bool,
    pub(crate) network_policy: NetworkPolicy,
    pub(crate) clear_env: bool,
    pub(crate) default_env: Arc<[EnvVar]>,
    pub(crate) extra_env: Arc<[EnvVar]>,
    pub(crate) availability: Availability,
    pub(crate) bwrap_program: Arc<Path>,
    pub(crate) shell: Box<Path>,
    pub(crate) static_args: Arc<[OsString]>,
}

impl Profile {
    /// Builds the public-bot defaults in one call.
    #[cfg(test)]
    pub(crate) fn public_bot_defaults(
        workspace: impl Into<Box<Path>>,
        synthetic_home: impl Into<Box<Path>>,
        cache_root: impl Into<Box<Path>>,
        tmp_backing: Option<TmpBacking>,
    ) -> Result<Self, LinuxBwrapError> {
        use super::Builder;
        Builder::public_bot(workspace, synthetic_home, cache_root, tmp_backing).build()
    }

    /// Builds the trusted-maintenance defaults in one call.
    #[cfg(test)]
    pub(crate) fn trusted_maintenance_defaults(
        workspace: impl Into<Box<Path>>,
        synthetic_home: impl Into<Box<Path>>,
        cache_root: impl Into<Box<Path>>,
        host_tmp: impl Into<Box<Path>>,
    ) -> Result<Self, LinuxBwrapError> {
        use super::Builder;
        Builder::trusted_maintenance(workspace, synthetic_home, cache_root, host_tmp).build()
    }

    /// Returns the preset used to create this profile, if any.
    ///
    /// Returns `None` if the profile was built without a preset.
    pub fn preset(&self) -> Option<Preset> {
        self.preset
    }

    /// Returns the host workspace path.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Returns the workspace path inside the sandbox.
    pub fn workspace_dest(&self) -> &Path {
        &self.workspace_dest
    }

    /// Returns the host synthetic home path.
    pub fn synthetic_home(&self) -> &Path {
        &self.synthetic_home
    }

    /// Returns the synthetic home path inside the sandbox.
    pub fn synthetic_home_dest(&self) -> &Path {
        &self.synthetic_home_dest
    }

    /// Returns the host cache root path.
    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }

    /// Returns the backing strategy for sandbox `/tmp`.
    pub fn tmp_backing(&self) -> &TmpBacking {
        &self.tmp_backing
    }

    /// Returns whether to mount the cache root.
    pub fn mount_cache_root(&self) -> bool {
        self.mount_cache_root
    }

    /// Returns the compatibility symlinks as a slice.
    pub fn compat_symlinks(&self) -> &[Symlink] {
        &self.compat_symlinks
    }

    /// Returns the read-only mounts as a slice.
    pub fn read_only_mounts(&self) -> &[Box<Path>] {
        &self.read_only_mounts
    }

    /// Returns the read-write mounts as a slice.
    pub fn read_write_mounts(&self) -> &[Box<Path>] {
        &self.read_write_mounts
    }

    /// Returns the tmpfs overlay paths as a slice.
    pub fn tmpfs_overlays(&self) -> &[Box<Path>] {
        &self.tmpfs_overlays
    }

    /// Returns the file overlays as a slice.
    ///
    /// Each overlay replaces a sandbox file with a read-only bind-mount of a
    /// host file, effectively masking the original content.
    pub fn file_overlays(&self) -> &[FileOverlay] {
        &self.file_overlays
    }

    /// Returns the credential file mounts as a slice.
    ///
    /// Make sure destination parent directories exist before launch.
    pub fn credential_file_mounts(&self) -> &[FileMount] {
        &self.credential_file_mounts
    }

    /// Returns whether the host root is mounted read-only.
    pub fn read_only_host_rootfs(&self) -> bool {
        self.read_only_host_rootfs
    }

    /// Returns the network policy.
    pub fn network_policy(&self) -> NetworkPolicy {
        self.network_policy
    }

    /// Returns whether inherited env vars are cleared.
    pub fn clear_env(&self) -> bool {
        self.clear_env
    }

    /// Returns the default environment variables as a slice.
    pub fn default_env(&self) -> &[EnvVar] {
        &self.default_env
    }

    /// Returns the extra environment variables as a slice.
    pub fn extra_env(&self) -> &[EnvVar] {
        &self.extra_env
    }

    /// Returns the availability state.
    pub fn availability(&self) -> &Availability {
        &self.availability
    }

    pub(crate) fn bwrap_program(&self) -> &Path {
        self.bwrap_program.as_ref()
    }

    pub(crate) fn shell(&self) -> &Path {
        &self.shell
    }

    pub(crate) fn static_args(&self) -> &[OsString] {
        &self.static_args
    }

    /// Translates a host working directory to the corresponding path inside the
    /// sandbox.
    ///
    /// Returns [`Cow::Borrowed`] when the path is returned unchanged;
    /// [`Cow::Owned`] only when a bind-mount prefix had to be rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`LinuxBwrapError::InvalidPath`] when `workdir` is a host path
    /// that the sandbox does not expose (not under any mounted prefix).
    #[inline]
    pub(crate) fn map_workdir_to_sandbox<'a>(
        &'a self,
        workdir: Option<&'a Path>,
    ) -> Result<Cow<'a, Path>, LinuxBwrapError> {
        let Some(dir) = workdir else {
            return Ok(Cow::Borrowed(self.workspace_dest()));
        };

        if let Some(mapping) = self.sandbox_layout().classify(dir) {
            return Ok(match mapping {
                PathMapping::SamePath => Cow::Borrowed(dir),
                PathMapping::Remap {
                    dest_prefix,
                    relative,
                } => join_mapped_path(dest_prefix, relative),
            });
        }

        Err(LinuxBwrapError::InvalidPath(format!(
            "working directory is not visible inside the linux sandbox: {}",
            dir.display()
        )))
    }

    fn sandbox_layout(&self) -> SandboxLayout<'_> {
        SandboxLayout {
            workspace: self.workspace(),
            workspace_dest: self.workspace_dest(),
            synthetic_home: self.synthetic_home(),
            synthetic_home_dest: self.synthetic_home_dest(),
            cache_root: self.cache_root(),
            mount_cache_root: self.mount_cache_root(),
            tmp_backing: self.tmp_backing(),
            read_only_host_rootfs: self.read_only_host_rootfs(),
            tmpfs_overlays: self.tmpfs_overlays(),
            file_overlays: self.file_overlays(),
            read_only_mounts: self.read_only_mounts(),
            read_write_mounts: self.read_write_mounts(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use std::ffi::OsString;

    fn profile_with_workspace_dest(workspace_dest: &str) -> Profile {
        Profile {
            preset: None,
            workspace: Box::from(Path::new("/host/workspace")),
            workspace_dest: Box::from(Path::new(workspace_dest)),
            synthetic_home: Box::from(Path::new("/host/home")),
            synthetic_home_dest: Box::from(Path::new("/sandbox/home")),
            cache_root: Box::from(Path::new("/cache")),
            tmp_backing: TmpBacking::Tmpfs,
            mount_cache_root: true,
            compat_symlinks: Arc::new([]),
            read_only_mounts: Arc::new([]),
            read_write_mounts: Arc::new([]),
            tmpfs_overlays: Arc::new([]),
            file_overlays: Arc::new([]),
            credential_file_mounts: Arc::new([]),
            read_only_host_rootfs: false,
            network_policy: NetworkPolicy::Disabled,
            clear_env: false,
            default_env: Arc::new([]),
            extra_env: Arc::new([]),
            availability: Availability::Unknown,
            bwrap_program: Arc::from(Box::from(Path::new("/usr/bin/bwrap"))),
            shell: Box::from(Path::new("/bin/sh")),
            static_args: Arc::<[OsString]>::from([]),
        }
    }

    #[test]
    fn map_workdir_to_sandbox_borrows_when_path_is_unchanged() {
        let dir = Path::new("/host/workspace/subdir");
        let profile = profile_with_workspace_dest("/host/workspace");

        match profile.map_workdir_to_sandbox(Some(dir)).unwrap() {
            Cow::Borrowed(mapped) => assert_eq!(mapped, dir),
            Cow::Owned(mapped) => panic!("expected borrowed path, got {}", mapped.display()),
        }
    }

    #[test]
    fn map_workdir_to_sandbox_allocates_only_for_rewritten_prefixes() {
        let dir = Path::new("/host/workspace/subdir");
        let profile = profile_with_workspace_dest("/workspace");

        match profile.map_workdir_to_sandbox(Some(dir)).unwrap() {
            Cow::Borrowed(mapped) => panic!("expected owned path, got {}", mapped.display()),
            Cow::Owned(mapped) => assert_eq!(mapped, Path::new("/workspace/subdir")),
        }
    }
}
