//! Builder for [`crate::profile::Profile`].
//!
//! Start with [`crate::profile::Builder::new`] for a blank builder, or use
//! [`crate::profile::Builder::public_bot`] or
//! [`crate::profile::Builder::trusted_maintenance`] for preset defaults.
//! Call [`crate::profile::Builder::build`] when you are done.

use super::layout::{join_mapped_path, PathMapping, SandboxLayout};
use super::types::{
    Availability, EnvVar, FileMount, FileOverlay, NetworkPolicy, Preset, Profile, Symlink,
    TmpBacking,
};
use super::validation::{
    ensure_cache_root_subdirs, validate_absolute_path, validate_directory_path, validate_env_vars,
    validate_file_overlays, validate_mount_paths, validate_symlinks, validate_tmp_backing,
    validate_tmpfs_overlays,
};
use crate::probe::{first_shell_candidate_with, resolve_backend_or_error_for};
use crate::LinuxBwrapError;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Builds a validated [`crate::profile::Profile`].
///
/// Start with [`Self::new`] for a blank builder, or use one of the preset
/// helpers. Then call [`Self::build`].
///
/// # Examples
///
/// Baseline builder:
/// ```no_run
/// use llm_coding_tools_bubblewrap::profile::{Builder, TmpBacking};
/// use std::path::Path;
///
/// fn main() -> Result<(), llm_coding_tools_bubblewrap::LinuxBwrapError> {
///     let profile = Builder::new(
///         Path::new("/host/workspace"),   // workspace: host directory mounted into the sandbox
///         Path::new("/tmp/home"),         // synthetic_home: host dir mounted as $HOME inside the sandbox
///         Path::new("/tmp/cache"),        // cache_root: host cache root used for sandbox cache/state dirs
///         TmpBacking::Tmpfs,              // tmp_backing: how sandbox /tmp is backed (RAM or host dir)
///     )
///     .build()?;
///
///     assert_eq!(profile.workspace(), Path::new("/host/workspace"));
///     Ok(())
/// }
/// ```
///
/// Public bot preset:
/// ```no_run
/// use llm_coding_tools_bubblewrap::profile::{Builder, TmpBacking};
/// use std::path::Path;
///
/// fn main() -> Result<(), llm_coding_tools_bubblewrap::LinuxBwrapError> {
///     let profile = Builder::public_bot(
///         Path::new("/host/workspace"),   // workspace: host directory mounted into the sandbox
///         Path::new("/tmp/home"),         // synthetic_home: host dir mounted as $HOME (/home/sandbox) inside the sandbox
///         Path::new("/tmp/cache"),        // cache_root: host cache root used for sandbox cache/state dirs
///         Some(TmpBacking::Tmpfs),        // tmp_backing: how sandbox /tmp is backed (RAM or host dir)
///     )
///     .build()?;
///
///     assert_eq!(profile.synthetic_home_dest(), Path::new("/home/sandbox"));
///     Ok(())
/// }
/// ```
///
/// # Notes
/// - `workspace`, `synthetic_home`, and `cache_root` are host paths.
/// - `tmp_backing` chooses memory-backed or host-backed `/tmp`.
/// - `build` validates profile-owned paths, resolves the `bwrap` executable,
///   resolves a visible host shell, and precomputes the static `bwrap` argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Builder {
    /// Preset that produced this builder, if any.
    pub(crate) preset: Option<Preset>,
    /// Host path to the workspace directory.
    pub(crate) workspace: Box<Path>,
    /// Where the workspace appears inside the sandbox (defaults to [`workspace`](Self::workspace)).
    pub(crate) workspace_dest: Box<Path>,
    /// Host path to the synthetic home directory.
    pub(crate) synthetic_home: Box<Path>,
    /// Where the synthetic home appears inside the sandbox (defaults to [`synthetic_home`](Self::synthetic_home)).
    pub(crate) synthetic_home_dest: Box<Path>,
    /// Host path to the cache root directory.
    pub(crate) cache_root: Box<Path>,
    /// Backing strategy for sandbox `/tmp` (`tmpfs` or host bind).
    pub(crate) tmp_backing: TmpBacking,
    /// Whether the cache root is bind-mounted inside the sandbox.
    pub(crate) mount_cache_root: bool,
    /// Compatibility symlinks created inside the sandbox (e.g. `/usr/bin/env`).
    pub(crate) compat_symlinks: Arc<[Symlink]>,
    /// Host paths mounted read-only inside the sandbox.
    pub(crate) read_only_mounts: Arc<[Box<Path>]>,
    /// Host paths mounted read-write inside the sandbox.
    pub(crate) read_write_mounts: Arc<[Box<Path>]>,
    /// Sandbox paths backed by `tmpfs` (writable, discarded on exit).
    pub(crate) tmpfs_overlays: Arc<[Box<Path>]>,
    /// Sandbox files replaced by a read-only bind-mount of a host file.
    pub(crate) file_overlays: Arc<[FileOverlay]>,
    /// Individual files mounted read-only for credential injection.
    pub(crate) credential_file_mounts: Arc<[FileMount]>,
    /// When `true`, the entire host rootfs is mounted read-only instead of individual read-only mounts.
    pub(crate) read_only_host_rootfs: bool,
    /// Controls whether the sandbox has network access.
    pub(crate) network_policy: NetworkPolicy,
    /// When `true`, inherited env vars are cleared before applying [`default_env`](Self::default_env) and [`extra_env`](Self::extra_env).
    pub(crate) clear_env: bool,
    /// Env vars always set (applied before [`extra_env`](Self::extra_env)).
    pub(crate) default_env: Arc<[EnvVar]>,
    /// Additional env vars set on top of [`default_env`](Self::default_env).
    pub(crate) extra_env: Arc<[EnvVar]>,
    /// Tracks whether `bwrap` is usable (checked during [`build`](Self::build)).
    pub(crate) availability: Availability,
}

impl Builder {
    /// Creates a new builder with baseline defaults and no preset.
    ///
    /// # Arguments
    /// - `workspace` - Host path to the workspace directory.
    /// - `synthetic_home` - Host path to the synthetic home directory.
    /// - `cache_root` - Host path to the cache root directory.
    /// - `tmp_backing` - How sandbox `/tmp` is mounted.
    ///
    /// # Defaults
    /// - `workspace_dest` and `synthetic_home_dest` are set to match the host paths.
    /// - `mount_cache_root` is `true`.
    /// - Mount and env lists start empty.
    /// - `network_policy` is `Disabled`.
    /// - `clear_env` is `false`.
    /// - `availability` is `Unknown`.
    pub fn new(
        workspace: impl Into<Box<Path>>,
        synthetic_home: impl Into<Box<Path>>,
        cache_root: impl Into<Box<Path>>,
        tmp_backing: TmpBacking,
    ) -> Self {
        let workspace = workspace.into();
        let synthetic_home = synthetic_home.into();

        Self {
            preset: None,
            workspace_dest: workspace.clone(),
            workspace,
            synthetic_home_dest: synthetic_home.clone(),
            synthetic_home,
            cache_root: cache_root.into(),
            tmp_backing,
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
        }
    }

    /// Sets the preset that produced this builder.
    ///
    /// This is an internal helper used by preset constructors.
    pub(crate) fn with_preset(mut self, preset: Preset) -> Self {
        self.preset = Some(preset);
        self
    }

    /// Consumes the builder and produces a ready-to-run [`Profile`].
    ///
    /// Ensures cache-root subdirectories exist, validates all builder fields,
    /// resolves the `bwrap` executable on the host, locates a shell visible
    /// inside the sandbox, and precomputes the static `bwrap` argument vector.
    ///
    /// # Returns
    ///
    /// A [`Profile`] carrying the resolved `bwrap` path, shell path, and
    /// prebuilt argument list — everything needed to launch the sandbox.
    ///
    /// # Errors
    ///
    /// Returns [`LinuxBwrapError`] when any of the following checks fail:
    ///
    /// - Cache-root subdirectory creation (depends on preset).
    /// - Path validation: host paths must exist and be absolute directories;
    ///   destination paths must be absolute; credential file sources must be
    ///   regular files inside the sandbox mount tree.
    /// - Environment variable names must not contain `=`.
    /// - Symlink targets and link paths must be absolute.
    /// - Tmpfs overlay paths must be absolute.
    /// - The `bwrap` backend must be available on the host.
    /// - At least one host shell (`bash` or `sh`) must be visible inside the
    ///   sandbox given the current mount configuration.
    pub fn build(self) -> Result<Profile, LinuxBwrapError> {
        ensure_cache_root_subdirs(self.mount_cache_root, self.cache_root.as_ref())?;
        validate_builder(&self)?;
        let bwrap_program = resolve_backend_or_error_for(self.preset, &self.availability)?;
        let shell = resolve_shell_for_builder(&self)?;
        let static_args = build_static_args(&self);

        Ok(Profile {
            preset: self.preset,
            workspace: self.workspace,
            workspace_dest: self.workspace_dest,
            synthetic_home: self.synthetic_home,
            synthetic_home_dest: self.synthetic_home_dest,
            cache_root: self.cache_root,
            tmp_backing: self.tmp_backing,
            mount_cache_root: self.mount_cache_root,
            compat_symlinks: self.compat_symlinks,
            read_only_mounts: self.read_only_mounts,
            read_write_mounts: self.read_write_mounts,
            tmpfs_overlays: self.tmpfs_overlays,
            file_overlays: self.file_overlays,
            credential_file_mounts: self.credential_file_mounts,
            read_only_host_rootfs: self.read_only_host_rootfs,
            network_policy: self.network_policy,
            clear_env: self.clear_env,
            default_env: self.default_env,
            extra_env: self.extra_env,
            availability: Availability::Available,
            bwrap_program,
            shell,
            static_args,
        })
    }

    /// Sets where the workspace appears inside the sandbox.
    pub fn with_workspace_dest(mut self, dest: impl Into<Box<Path>>) -> Self {
        self.workspace_dest = dest.into();
        self
    }

    /// Sets where the synthetic home appears inside the sandbox.
    pub fn with_synthetic_home_dest(mut self, dest: impl Into<Box<Path>>) -> Self {
        self.synthetic_home_dest = dest.into();
        self
    }

    /// Sets whether to mount the cache root.
    pub fn with_mount_cache_root(mut self, mount_cache_root: bool) -> Self {
        self.mount_cache_root = mount_cache_root;
        self
    }

    /// Sets the backing strategy for sandbox `/tmp`.
    pub fn with_tmp_backing(mut self, tmp_backing: TmpBacking) -> Self {
        self.tmp_backing = tmp_backing;
        self
    }

    /// Replaces the compatibility symlink list.
    pub fn with_compat_symlinks(mut self, compat_symlinks: impl Into<Arc<[Symlink]>>) -> Self {
        self.compat_symlinks = compat_symlinks.into();
        self
    }

    /// Replaces the read-only mount list.
    pub fn with_read_only_mounts(mut self, mounts: impl Into<Arc<[Box<Path>]>>) -> Self {
        self.read_only_mounts = mounts.into();
        self
    }

    /// Replaces the read-write mount list.
    pub fn with_read_write_mounts(mut self, mounts: impl Into<Arc<[Box<Path>]>>) -> Self {
        self.read_write_mounts = mounts.into();
        self
    }

    /// Replaces the tmpfs overlay list.
    pub fn with_tmpfs_overlays(mut self, mounts: impl Into<Arc<[Box<Path>]>>) -> Self {
        self.tmpfs_overlays = mounts.into();
        self
    }

    /// Replaces the file overlay list.
    ///
    /// Each overlay replaces a sandbox file with a read-only bind-mount of
    /// the specified host source file.
    pub fn with_file_overlays(mut self, overlays: impl Into<Arc<[FileOverlay]>>) -> Self {
        self.file_overlays = overlays.into();
        self
    }

    /// Replaces the credential file mount list.
    pub fn with_credential_file_mounts(mut self, mounts: impl Into<Arc<[FileMount]>>) -> Self {
        self.credential_file_mounts = mounts.into();
        self
    }

    /// Sets whether to mount the host root read-only.
    pub fn with_read_only_host_rootfs(mut self, enabled: bool) -> Self {
        self.read_only_host_rootfs = enabled;
        self
    }

    /// Sets the network policy.
    pub fn with_network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = policy;
        self
    }

    /// Sets whether to clear inherited env vars.
    pub fn with_clear_env(mut self, clear: bool) -> Self {
        self.clear_env = clear;
        self
    }

    /// Replaces the default env var list.
    pub fn with_default_env(mut self, env: impl Into<Arc<[EnvVar]>>) -> Self {
        self.default_env = env.into();
        self
    }

    /// Replaces the extra env var list.
    pub fn with_extra_env(mut self, env: impl Into<Arc<[EnvVar]>>) -> Self {
        self.extra_env = env.into();
        self
    }

    /// Sets the stored availability state.
    pub fn with_availability(mut self, availability: Availability) -> Self {
        self.availability = availability;
        self
    }
}

fn validate_builder(builder: &Builder) -> Result<(), LinuxBwrapError> {
    validate_directory_path(builder.workspace.as_ref(), "workspace host directory")?;
    validate_directory_path(
        builder.synthetic_home.as_ref(),
        "synthetic home host directory",
    )?;
    validate_absolute_path(builder.cache_root.as_ref(), "cache root host path")?;
    if builder.mount_cache_root {
        validate_directory_path(builder.cache_root.as_ref(), "cache root host directory")?;
    }

    validate_absolute_path(builder.workspace_dest.as_ref(), "workspace destination")?;
    validate_absolute_path(
        builder.synthetic_home_dest.as_ref(),
        "synthetic home destination",
    )?;
    validate_tmp_backing(&builder.tmp_backing)?;
    validate_mount_paths(&builder.read_only_mounts, "read-only mount source")?;
    validate_mount_paths(&builder.read_write_mounts, "read-write mount source")?;
    validate_tmpfs_overlays(&builder.tmpfs_overlays)?;
    validate_file_overlays(&builder.file_overlays)?;
    validate_symlinks(&builder.compat_symlinks)?;
    validate_env_vars(builder.default_env.as_ref(), "default")?;
    validate_env_vars(builder.extra_env.as_ref(), "extra")?;
    validate_credential_file_mounts(builder)?;
    Ok(())
}

fn validate_credential_file_mounts(builder: &Builder) -> Result<(), LinuxBwrapError> {
    for mount in builder.credential_file_mounts.iter() {
        validate_absolute_path(mount.source(), "credential file source")?;
        validate_absolute_path(mount.dest(), "credential file destination")?;

        let metadata = fs::metadata(mount.source()).map_err(|error| {
            LinuxBwrapError::InvalidPath(format!(
                "credential file source must exist and be readable: {} ({error})",
                mount.source().display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "credential file source must be a regular file: {}",
                mount.source().display()
            )));
        }
        if !credential_dest_is_allowed(builder, mount.dest()) {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "credential file destination must stay within the synthetic home, workspace, or cache root: {}",
                mount.dest().display()
            )));
        }
    }

    Ok(())
}

fn credential_dest_is_allowed(builder: &Builder, dest: &Path) -> bool {
    dest.starts_with(builder.synthetic_home_dest.as_ref())
        || dest.starts_with(builder.workspace_dest.as_ref())
        || (builder.mount_cache_root && dest.starts_with(builder.cache_root.as_ref()))
}

fn resolve_shell_for_builder(builder: &Builder) -> Result<Box<Path>, LinuxBwrapError> {
    let layout = builder_sandbox_layout(builder);
    if let Some((_host_shell, sandbox_path)) = first_shell_candidate_with(|shell| {
        layout.classify(shell).map(|mapping| match mapping {
            PathMapping::SamePath => shell.to_path_buf(),
            PathMapping::Remap {
                dest_prefix,
                relative,
            } => join_mapped_path(dest_prefix, relative).into_owned(),
        })
    }) {
        return Ok(sandbox_path.into_boxed_path());
    }

    Err(LinuxBwrapError::Execution(
        "no usable host shell is visible inside the linux sandbox; expected a system `bash` or `sh` mounted by the selected profile"
            .to_string(),
    ))
}

fn builder_sandbox_layout(builder: &Builder) -> SandboxLayout<'_> {
    SandboxLayout {
        workspace: builder.workspace.as_ref(),
        workspace_dest: builder.workspace_dest.as_ref(),
        synthetic_home: builder.synthetic_home.as_ref(),
        synthetic_home_dest: builder.synthetic_home_dest.as_ref(),
        cache_root: builder.cache_root.as_ref(),
        mount_cache_root: builder.mount_cache_root,
        tmp_backing: &builder.tmp_backing,
        read_only_host_rootfs: builder.read_only_host_rootfs,
        tmpfs_overlays: builder.tmpfs_overlays.as_ref(),
        file_overlays: builder.file_overlays.as_ref(),
        read_only_mounts: builder.read_only_mounts.as_ref(),
        read_write_mounts: builder.read_write_mounts.as_ref(),
    }
}

fn build_static_args(builder: &Builder) -> Arc<[OsString]> {
    let mut args = Vec::with_capacity(arg_capacity_for(builder));

    args.extend([
        OsString::from("--die-with-parent"),
        OsString::from("--new-session"),
    ]);

    if matches!(builder.network_policy, NetworkPolicy::Disabled) {
        args.push(OsString::from("--unshare-net"));
    }
    if builder.clear_env {
        args.push(OsString::from("--clearenv"));
    }
    push_env_args(&mut args, builder.default_env.as_ref());
    push_env_args(&mut args, builder.extra_env.as_ref());

    if builder.read_only_host_rootfs {
        push_bind(&mut args, "--ro-bind", Path::new("/"), Path::new("/"));
    }
    push_tmpfs_mounts(&mut args, builder.tmpfs_overlays.as_ref());
    push_file_overlay_mounts(&mut args, builder.file_overlays.as_ref());
    if !builder.read_only_host_rootfs {
        push_same_path_binds(&mut args, "--ro-bind", builder.read_only_mounts.as_ref());
    }
    push_symlinks(&mut args, builder.compat_symlinks.as_ref());
    args.extend([
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--proc"),
        OsString::from("/proc"),
    ]);
    push_tmp_mount(&mut args, &builder.tmp_backing);
    push_bind(
        &mut args,
        "--bind",
        builder.synthetic_home.as_ref(),
        builder.synthetic_home_dest.as_ref(),
    );
    if builder.mount_cache_root {
        push_bind(
            &mut args,
            "--bind",
            builder.cache_root.as_ref(),
            builder.cache_root.as_ref(),
        );
    }
    push_bind(
        &mut args,
        "--bind",
        builder.workspace.as_ref(),
        builder.workspace_dest.as_ref(),
    );
    push_same_path_binds(&mut args, "--bind", builder.read_write_mounts.as_ref());
    push_file_mounts(&mut args, builder.credential_file_mounts.as_ref());

    Arc::from(args)
}

fn arg_capacity_for(builder: &Builder) -> usize {
    let env_count = builder.default_env.len() + builder.extra_env.len();
    let ro_slots = if builder.read_only_host_rootfs {
        3
    } else {
        builder.read_only_mounts.len() * 3
    };
    let mount_slots = ro_slots
        + builder.read_write_mounts.len() * 3
        + builder.credential_file_mounts.len() * 3
        + builder.compat_symlinks.len() * 3
        + builder.tmpfs_overlays.len() * 2
        + builder.file_overlays.len() * 3
        + usize::from(builder.mount_cache_root) * 3;
    let tmp_slots = match builder.tmp_backing {
        TmpBacking::Tmpfs => 2,
        TmpBacking::BindHost(_) => 3,
    };
    let fixed_slots = 12
        + usize::from(builder.clear_env)
        + usize::from(matches!(builder.network_policy, NetworkPolicy::Disabled));

    fixed_slots + env_count * 3 + mount_slots + tmp_slots
}

fn push_bind(args: &mut Vec<OsString>, flag: &str, source: &Path, dest: &Path) {
    args.push(OsString::from(flag));
    args.push(source.as_os_str().into());
    args.push(dest.as_os_str().into());
}

fn push_symlinks(args: &mut Vec<OsString>, symlinks: &[Symlink]) {
    for symlink in symlinks {
        args.push(OsString::from("--symlink"));
        args.push(OsString::from(symlink.target()));
        args.push(symlink.link_path().as_os_str().into());
    }
}

fn push_env_args(args: &mut Vec<OsString>, env_vars: &[EnvVar]) {
    for var in env_vars {
        args.push(OsString::from("--setenv"));
        args.push(OsString::from(var.name()));
        args.push(OsString::from(var.value()));
    }
}

fn push_same_path_bind(args: &mut Vec<OsString>, flag: &str, path: &Path) {
    args.push(OsString::from(flag));
    args.push(path.as_os_str().into());
    args.push(path.as_os_str().into());
}

fn push_same_path_binds(args: &mut Vec<OsString>, flag: &str, paths: &[Box<Path>]) {
    for path in paths {
        push_same_path_bind(args, flag, path);
    }
}

fn push_tmpfs_mounts(args: &mut Vec<OsString>, paths: &[Box<Path>]) {
    for path in paths {
        args.push(OsString::from("--tmpfs"));
        args.push(path.as_os_str().into());
    }
}

fn push_file_overlay_mounts(args: &mut Vec<OsString>, overlays: &[FileOverlay]) {
    for overlay in overlays {
        push_bind(args, "--ro-bind", overlay.source(), overlay.dest());
    }
}

fn push_tmp_mount(args: &mut Vec<OsString>, tmp_backing: &TmpBacking) {
    match tmp_backing {
        TmpBacking::Tmpfs => {
            args.push(OsString::from("--tmpfs"));
            args.push(OsString::from("/tmp"));
        }
        TmpBacking::BindHost(host_dir) => push_bind(args, "--bind", host_dir, Path::new("/tmp")),
    }
}

fn push_file_mounts(args: &mut Vec<OsString>, mounts: &[FileMount]) {
    for mount in mounts {
        push_bind(args, "--ro-bind", mount.source(), mount.dest());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{SandboxDirs, SandboxFixture};
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    #[serial]
    fn empty_builder_builds_identity_destinations_and_empty_collections() {
        let fixture = SandboxFixture::new("exit 0");
        let workspace = fixture.temp_path().to_path_buf();
        let home = fixture.home().to_path_buf();
        let cache = fixture.cache().to_path_buf();

        let profile = Builder::new(&*workspace, &*home, &*cache, TmpBacking::Tmpfs)
            .build()
            .unwrap();

        assert_eq!(profile.workspace(), workspace.as_path());
        assert_eq!(profile.workspace_dest(), workspace.as_path());
        assert_eq!(profile.synthetic_home(), home.as_path());
        assert_eq!(profile.synthetic_home_dest(), home.as_path());
        assert_eq!(profile.cache_root(), cache.as_path());
        assert_eq!(profile.tmp_backing(), &TmpBacking::Tmpfs);
        assert!(profile.compat_symlinks().is_empty());
        assert!(profile.read_only_mounts().is_empty());
        assert!(profile.read_write_mounts().is_empty());
        assert!(profile.tmpfs_overlays().is_empty());
        assert!(profile.credential_file_mounts().is_empty());
        assert!(profile.default_env().is_empty());
        assert!(profile.extra_env().is_empty());
        assert!(profile.mount_cache_root());
        assert!(!profile.read_only_host_rootfs());
        assert!(!profile.clear_env());
        assert_eq!(profile.network_policy(), NetworkPolicy::Disabled);
        assert_eq!(profile.preset(), None);
        assert!(profile.availability().is_available());
        assert!(profile.bwrap_program().ends_with("bwrap"));
        assert!(profile.shell().ends_with("bash") || profile.shell().ends_with("sh"));
        assert!(!profile.static_args().is_empty());
    }

    #[test]
    #[serial]
    fn preset_builder_can_still_be_customized_before_build() {
        let fixture = SandboxFixture::new("exit 0");
        let workspace = fixture.temp_path().to_path_buf();
        let host_tmp = fixture.make_dir("host-tmp");

        let profile = Builder::new(
            &*workspace,
            fixture.home(),
            fixture.cache(),
            TmpBacking::Tmpfs,
        )
        .with_preset(Preset::PublicBot)
        .with_mount_cache_root(true)
        .with_tmp_backing(TmpBacking::BindHost(host_tmp.clone().into_boxed_path()))
        .with_extra_env(Arc::from([EnvVar::new("FOO", "bar")]))
        .build()
        .unwrap();

        assert_eq!(profile.preset(), Some(Preset::PublicBot));
        assert!(profile.mount_cache_root());
        assert_eq!(
            profile.tmp_backing(),
            &TmpBacking::BindHost(host_tmp.into_boxed_path())
        );
        assert_eq!(profile.extra_env().len(), 1);
        assert_eq!(profile.extra_env()[0].name(), "FOO");
        assert_eq!(profile.extra_env()[0].value(), "bar");
    }

    #[test]
    #[serial]
    fn arg_capacity_for_matches_actual_push_count() {
        let fixture = SandboxFixture::new("exit 0");
        let workspace = fixture.temp_path().to_path_buf();
        let home = fixture.home().to_path_buf();
        let cache = fixture.cache().to_path_buf();
        let host_tmp = fixture.make_dir("host-tmp");
        let cred_file = fixture.temp_path().join("cred.txt");
        fs::write(&cred_file, "secret").unwrap();

        let builder = Builder::new(
            &*workspace,
            &*home,
            &*cache,
            TmpBacking::BindHost(host_tmp.into_boxed_path()),
        )
        .with_read_only_host_rootfs(true)
        .with_network_policy(NetworkPolicy::Disabled)
        .with_clear_env(true)
        .with_default_env(Arc::from([EnvVar::new("A", "1"), EnvVar::new("B", "2")]))
        .with_extra_env(Arc::from([EnvVar::new("C", "3")]))
        .with_compat_symlinks(Arc::from([Symlink::new(
            "/usr/bin/python3",
            Path::new("/usr/bin/python3"),
        )]))
        .with_tmpfs_overlays(Arc::from([Path::new("/run").into()]))
        .with_credential_file_mounts(Arc::from([FileMount::new(
            cred_file.into_boxed_path(),
            Path::new("/sandbox/cred.txt"),
        )]))
        .with_read_write_mounts(Arc::from([Path::new("/data").into()]));

        let capacity = arg_capacity_for(&builder);
        let args = build_static_args(&builder);
        assert_eq!(args.len(), capacity);
    }

    #[test]
    #[serial]
    fn arg_capacity_for_minimal_builder() {
        let fixture = SandboxFixture::new("exit 0");
        let workspace = fixture.temp_path().to_path_buf();
        let home = fixture.home().to_path_buf();
        let cache = fixture.cache().to_path_buf();

        let builder = Builder::new(&*workspace, &*home, &*cache, TmpBacking::Tmpfs);

        let capacity = arg_capacity_for(&builder);
        let args = build_static_args(&builder);
        assert_eq!(args.len(), capacity);
    }

    #[test]
    fn build_rejects_invalid_env_var_names() {
        let dirs = SandboxDirs::new();

        let err = Builder::new(
            dirs.workspace(),
            dirs.home(),
            dirs.cache(),
            TmpBacking::Tmpfs,
        )
        .with_default_env(Arc::from([EnvVar::new("BAD=NAME", "value")]))
        .build()
        .unwrap_err();

        assert!(format!("{err}").contains("must not contain '='"));
    }

    #[test]
    fn build_rejects_unavailable_backend_reason() {
        let dirs = SandboxDirs::new();

        let err = Builder::new(
            dirs.workspace(),
            dirs.home(),
            dirs.cache(),
            TmpBacking::Tmpfs,
        )
        .with_availability(Availability::unavailable("bwrap blocked by policy"))
        .build()
        .unwrap_err();

        assert!(format!("{err}").contains("bwrap blocked by policy"));
        assert!(format!("{err}").contains("unavailable"));
    }

    #[test]
    fn public_bot_preset_rejects_nonexistent_workspace_directory() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let cache = temp.path().join("cache");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&cache).unwrap();
        let workspace = temp.path().join("workspace_does_not_exist");

        let err = Builder::public_bot(&*workspace, &*home, &*cache, Some(TmpBacking::Tmpfs))
            .with_availability(Availability::Available)
            .build()
            .unwrap_err();

        assert!(format!("{err}").contains("workspace host directory"));
    }
}
