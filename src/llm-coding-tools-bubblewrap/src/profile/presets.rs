//! Preset helpers for common sandbox setups.
//!
//! Use [`crate::profile::Builder::public_bot`] for untrusted input.
//! Use [`crate::profile::Builder::trusted_maintenance`] for trusted jobs.
//! Both return a [`crate::profile::Builder`], so you can still change settings
//! before calling [`crate::profile::Builder::build`].

use super::builder::Builder;
use super::types::{EnvVar, FileOverlay, NetworkPolicy, Preset, Symlink, TmpBacking};
use crate::path_util::normalize_path;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl Builder {
    /// Creates the public-bot preset builder.
    ///
    /// Good default for untrusted input.
    ///
    /// - mounts selected system paths, the workspace, and a synthetic home
    /// - clears the inherited env and sets a cleaned `PATH` plus `HOME`
    /// - disables network
    /// - uses `/workspace` and `/home/sandbox` inside the sandbox
    /// - does not mount the cache root
    /// - uses `Tmpfs` for `/tmp` unless you pass `Some(...)`
    ///
    /// It hides the real home directory, `/etc`, and other unrelated host
    /// paths.
    ///
    /// # Arguments
    /// - `workspace` - Host path to the workspace directory.
    /// - `synthetic_home` - Host path to the synthetic home directory.
    /// - `cache_root` - Host path to the cache root directory (not mounted by default).
    /// - `tmp_backing` - Optional `/tmp` backing. Defaults to `Tmpfs`.
    pub fn public_bot(
        workspace: impl Into<Box<Path>>,
        synthetic_home: impl Into<Box<Path>>,
        cache_root: impl Into<Box<Path>>,
        tmp_backing: Option<TmpBacking>,
    ) -> Self {
        Self::new(
            workspace,
            synthetic_home,
            cache_root,
            tmp_backing.unwrap_or(TmpBacking::Tmpfs),
        )
        .with_preset(Preset::PublicBot)
        .with_workspace_dest(Path::new(WORKSPACE_DEST))
        .with_synthetic_home_dest(Path::new(SYNTHETIC_HOME_DEST))
        .with_mount_cache_root(false)
        .with_clear_env(true)
        .with_default_env(Arc::from([
            EnvVar::new("PATH", inherited_path(Preset::PublicBot)),
            EnvVar::new("HOME", SYNTHETIC_HOME_DEST),
        ]))
        .with_read_only_mounts(public_bot_read_only_mounts())
        .with_compat_symlinks(public_bot_compat_symlinks())
    }

    /// Creates the trusted-maintenance preset builder.
    ///
    /// Use this only for trusted jobs. Network stays enabled, so a command can
    /// send out any data it can read.
    ///
    /// - mounts the host root read-only
    /// - overlays tmpfs on `/home`; masks `/etc/shadow` with an empty file
    /// - uses a synthetic home at `/home/sandbox`
    /// - clears the inherited env and sets a cleaned `PATH`, `HOME`, `TMPDIR`, and `XDG_*`
    /// - keeps network enabled
    /// - bind-mounts the `host_tmp` directory at `/tmp`
    ///
    /// Writable state stays in the synthetic home, workspace, cache root, and
    /// tmpfs overlays. `/etc/shadow` is masked by a read-only bind-mount of an
    /// empty regular file so password hashes are not exposed.
    ///
    /// # Arguments
    /// - `workspace` - Host path to the workspace directory.
    /// - `synthetic_home` - Host path to the synthetic home directory.
    /// - `cache_root` - Host path to the cache root directory. Missing
    ///   `xdg-cache` and `xdg-state` subdirectories are created during `build()`.
    /// - `host_tmp` - Host path to mount at sandbox `/tmp` (must exist).
    pub fn trusted_maintenance(
        workspace: impl Into<Box<Path>>,
        synthetic_home: impl Into<Box<Path>>,
        cache_root: impl Into<Box<Path>>,
        host_tmp: impl Into<Box<Path>>,
    ) -> Self {
        let cache_root = cache_root.into();
        let tmp_backing = TmpBacking::BindHost(host_tmp.into());

        Self::new(workspace, synthetic_home, cache_root.clone(), tmp_backing)
            .with_preset(Preset::TrustedMaintenance)
            .with_synthetic_home_dest(Path::new(SYNTHETIC_HOME_DEST))
            .with_read_only_host_rootfs(true)
            .with_tmpfs_overlays(Arc::from([Box::from(Path::new("/home"))]))
            .with_file_overlays(Arc::from([FileOverlay::new(
                Path::new("/dev/null"),
                Path::new("/etc/shadow"),
            )]))
            .with_clear_env(true)
            .with_network_policy(NetworkPolicy::Enabled)
            .with_default_env(Arc::from([
                EnvVar::new("PATH", inherited_path(Preset::TrustedMaintenance)),
                EnvVar::new("HOME", SYNTHETIC_HOME_DEST),
                EnvVar::new("TMPDIR", "/tmp"),
                EnvVar::new(
                    "XDG_CACHE_HOME",
                    cache_root.join("xdg-cache").to_string_lossy().into_owned(),
                ),
                EnvVar::new("XDG_CONFIG_HOME", SYNTHETIC_HOME_CONFIG),
                EnvVar::new(
                    "XDG_STATE_HOME",
                    cache_root.join("xdg-state").to_string_lossy().into_owned(),
                ),
            ]))
    }
}

const SYNTHETIC_HOME_DEST: &str = "/home/sandbox";
const SYNTHETIC_HOME_CONFIG: &str = "/home/sandbox/.config";
const WORKSPACE_DEST: &str = "/workspace";

const DEFAULT_SANDBOX_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin";
const PUBLIC_BOT_PREFIXES: &[&str] = &[
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/lib",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/run/current-system/sw",
    "/nix/store",
    "/nix/var/nix/profiles/default",
];
const TRUSTED_DENY_PREFIXES: &[&str] = &[
    "/home",
    "/root",
    "/tmp",
    "/var/tmp",
    "/run/user",
    "/run/wrappers/bin",
    "/etc/profiles/per-user",
];

/// Builds a filtered `PATH` string from the host environment for the given [`Preset`].
///
/// Each host entry is checked with [`path_entry_allowed`]; entries that fail the
/// check, are empty, or are duplicates are dropped. Falls back to
/// [`DEFAULT_SANDBOX_PATH`] when the host `PATH` is unset or all entries are
/// filtered out.
fn inherited_path(preset: Preset) -> String {
    let Some(path) = std::env::var_os("PATH") else {
        return DEFAULT_SANDBOX_PATH.to_string();
    };

    // Preallocate based on upper bound: number of separators + 1
    let path_bytes = path.as_encoded_bytes();
    let capacity = path_bytes.iter().filter(|&&b| b == b':').count() + 1;
    let mut entries = Vec::with_capacity(capacity);
    let mut seen = HashSet::with_capacity(capacity);
    for entry in std::env::split_paths(&path) {
        let entry = normalize_path(&entry);
        if !path_entry_allowed(preset, &entry) {
            continue;
        }
        let value = entry.to_string_lossy();
        if value.is_empty() {
            continue;
        }
        let value = value.into_owned();
        if !seen.insert(value.clone()) {
            continue;
        }
        entries.push(value);
    }

    if entries.is_empty() {
        DEFAULT_SANDBOX_PATH.to_string()
    } else {
        entries.join(":")
    }
}

/// Checks whether a `PATH` entry is safe to include for the given [`Preset`].
///
/// The caller must pass an absolute, normalized path. For [`Preset::PublicBot`]
/// only entries under [`PUBLIC_BOT_PREFIXES`] are allowed. For
/// [`Preset::TrustedMaintenance`] everything is allowed except entries under
/// [`TRUSTED_DENY_PREFIXES`].
fn path_entry_allowed(preset: Preset, entry: &Path) -> bool {
    match preset {
        Preset::PublicBot => PUBLIC_BOT_PREFIXES
            .iter()
            .any(|prefix| entry.starts_with(prefix)),
        Preset::TrustedMaintenance => {
            entry.is_absolute()
                && !TRUSTED_DENY_PREFIXES
                    .iter()
                    .any(|prefix| entry.starts_with(prefix))
        }
    }
}

/// Collects host directories to mount read-only for [`Preset::PublicBot`].
///
/// Checks each prefix in [`PUBLIC_BOT_PREFIXES`] against the host filesystem
/// and includes only those that exist.
fn public_bot_read_only_mounts() -> Arc<[Box<Path>]> {
    let mut mounts = Vec::with_capacity(PUBLIC_BOT_PREFIXES.len());
    for path in PUBLIC_BOT_PREFIXES {
        let path = PathBuf::from(path);
        if path.exists() {
            mounts.push(path.into_boxed_path());
        }
    }
    mounts.into()
}

/// Collects compatibility symlinks for [`Preset::PublicBot`].
///
/// On systems without a merged `/usr` layout, `/bin`, `/lib`, and `/sbin` may
/// not exist as symlinks to their `/usr` counterparts. This function checks
/// each candidate and includes only those where the link path is absent and
/// the target directory exists on the host.
fn public_bot_compat_symlinks() -> Arc<[Symlink]> {
    let mut symlinks = Vec::with_capacity(3);
    for (target, link_path, required_path) in [
        ("usr/bin", "/bin", "/usr/bin"),
        ("usr/lib", "/lib", "/usr/lib"),
        ("usr/sbin", "/sbin", "/usr/sbin"),
    ] {
        if !Path::new(link_path).exists() && Path::new(required_path).exists() {
            symlinks.push(Symlink::new(target, Path::new(link_path)));
        }
    }
    symlinks.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::NetworkPolicy;
    use crate::test_helpers::SandboxFixture;
    use serial_test::serial;
    use std::env;

    #[test]
    #[serial]
    fn public_bot_and_trusted_maintenance_differ_on_security_axes() {
        let fixture = SandboxFixture::new("exit 0");
        let host_tmp = fixture.make_dir("host-tmp");

        let public = Builder::public_bot(
            fixture.workspace(),
            fixture.home(),
            fixture.cache(),
            Some(TmpBacking::Tmpfs),
        )
        .build()
        .unwrap();

        let trusted = Builder::trusted_maintenance(
            fixture.workspace(),
            fixture.home(),
            fixture.cache(),
            host_tmp.as_path(),
        )
        .build()
        .unwrap();

        assert_eq!(public.preset(), Some(Preset::PublicBot));
        assert_eq!(trusted.preset(), Some(Preset::TrustedMaintenance));

        assert!(!public.read_only_host_rootfs());
        assert!(trusted.read_only_host_rootfs());

        assert!(public.tmpfs_overlays().is_empty());
        assert!(!trusted.tmpfs_overlays().is_empty());
        assert!(public.file_overlays().is_empty());
        assert!(!trusted.file_overlays().is_empty());

        assert_eq!(public.network_policy(), NetworkPolicy::Disabled);
        assert_eq!(trusted.network_policy(), NetworkPolicy::Enabled);

        assert!(!public.mount_cache_root());
        assert!(trusted.mount_cache_root());

        assert!(public.clear_env());
        assert!(trusted.clear_env());

        assert!(public.credential_file_mounts().is_empty());
        assert!(public.extra_env().is_empty());
        assert!(trusted.credential_file_mounts().is_empty());
        assert!(trusted.extra_env().is_empty());
    }

    #[test]
    #[serial]
    fn public_bot_path_filters_user_and_temp_entries() {
        let fixture = SandboxFixture::new("exit 0");
        unsafe {
            env::set_var(
                "PATH",
                format!(
                    "/home/alice/.cargo/bin:{}:/tmp/test-bin:/run/current-system/sw/bin:/nix/store/demo/bin",
                    fixture.temp_path().display()
                ),
            )
        };

        let profile = Builder::public_bot(
            fixture.workspace(),
            fixture.home(),
            fixture.cache(),
            Some(TmpBacking::Tmpfs),
        )
        .build()
        .unwrap();

        let path = profile.default_env()[0].value();
        assert!(path.contains(":"));
        assert!(!path.contains("/home/alice/.cargo/bin"));
        assert!(!path.contains("/tmp/test-bin"));
    }

    #[test]
    #[serial]
    fn trusted_maintenance_path_filters_hidden_and_volatile_entries() {
        let fixture = SandboxFixture::new("exit 0");
        let host_tmp = fixture.make_dir("host-tmp");
        unsafe {
            env::set_var(
                "PATH",
                format!(
                    "{}:/home/alice/.nix-profile/bin:/run/user/1000/bin:/nix/store/demo/bin:/opt/tool/bin",
                    fixture.temp_path().display()
                ),
            )
        };

        let profile = Builder::trusted_maintenance(
            fixture.workspace(),
            fixture.home(),
            fixture.cache(),
            host_tmp.as_path(),
        )
        .build()
        .unwrap();

        let path = profile
            .default_env()
            .iter()
            .find(|var| var.name() == "PATH")
            .expect("PATH should be present")
            .value();
        assert!(path.contains(":"));
        assert!(!path.contains("/home/alice/.nix-profile/bin"));
        assert!(!path.contains("/run/user/1000/bin"));
    }
}
