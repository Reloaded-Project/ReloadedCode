//! Given a host path, determine whether it is reachable inside the sandbox
//! and where it appears.
//!
//! A bubblewrap sandbox hides the host filesystem and re-exposes only
//! specific directories (workspace, home, `/tmp`, extra mounts).
//!
//! Callers need this to pick a usable shell, translate a working directory,
//! or validate a user-supplied path before launching a sandboxed command.

use super::types::{FileOverlay, TmpBacking};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Snapshot of the path-mapping rules that determine which host paths are
/// reachable inside the sandbox and where they appear.
///
/// Constructed from either a [`Profile`][super::types::Profile] or an
/// in-progress [`Builder`][super::builder::Builder] and passed to
/// [`SandboxLayout::classify`].
#[derive(Clone, Copy)]
pub(crate) struct SandboxLayout<'a> {
    pub(crate) workspace: &'a Path,
    pub(crate) workspace_dest: &'a Path,
    pub(crate) synthetic_home: &'a Path,
    pub(crate) synthetic_home_dest: &'a Path,
    pub(crate) cache_root: &'a Path,
    pub(crate) mount_cache_root: bool,
    pub(crate) tmp_backing: &'a TmpBacking,
    pub(crate) read_only_host_rootfs: bool,
    pub(crate) tmpfs_overlays: &'a [Box<Path>],
    pub(crate) file_overlays: &'a [FileOverlay],
    pub(crate) read_only_mounts: &'a [Box<Path>],
    pub(crate) read_write_mounts: &'a [Box<Path>],
}

/// Describes where a host path ends up inside the sandbox.
pub(crate) enum PathMapping<'config, 'path> {
    /// The path appears at the same absolute location in the sandbox.
    SamePath,
    /// The path appears under a different prefix inside the sandbox.
    ///
    /// The sandbox path is `dest_prefix` joined with `relative`.
    Remap {
        dest_prefix: &'config Path,
        relative: &'path Path,
    },
}

impl<'config> SandboxLayout<'config> {
    /// Determines how `entry` appears inside the sandbox, if at all.
    ///
    /// Returns [`Some`] with the mapping when the host path is reachable,
    /// [`None`] when it is hidden (not mounted, or covered by a tmpfs overlay).
    /// Relative paths are always `None`.
    pub(crate) fn classify(self, entry: &Path) -> Option<PathMapping<'config, '_>> {
        // Relative paths have no fixed location inside the sandbox.
        if !entry.is_absolute() {
            return None;
        }
        // Workspace: the project directory the sandbox is allowed to read/write.
        if let Some(mapping) = map_prefix(entry, self.workspace, self.workspace_dest) {
            return Some(mapping);
        }
        // Synthetic home: the sandbox's $HOME, bind-mounted from the host.
        if let Some(mapping) = map_prefix(entry, self.synthetic_home, self.synthetic_home_dest) {
            return Some(mapping);
        }
        // Caller-provided tmp directory, remapped to /tmp inside the sandbox.
        if let TmpBacking::BindHost(host_dir) = self.tmp_backing {
            if let Some(mapping) = map_prefix(entry, host_dir, Path::new("/tmp")) {
                return Some(mapping);
            }
        }
        // Cache root: mounted read-write when enabled.
        if self.mount_cache_root && entry.starts_with(self.cache_root) {
            return Some(PathMapping::SamePath);
        }
        // Extra mounts: user-specified read-only and read-write bind mounts.
        // Both appear after tmpfs/file overlays in the bwrap arg list, so they
        // always take precedence over any overlay at overlapping paths.
        if self
            .read_only_mounts
            .iter()
            .chain(self.read_write_mounts.iter())
            .any(|mount| entry.starts_with(mount.as_ref()))
        {
            return Some(PathMapping::SamePath);
        }
        // Read-only host rootfs: everything else is visible unless a tmpfs
        // overlay hides it.
        if self.read_only_host_rootfs
            && !path_hidden_by_overlay(
                self.tmpfs_overlays,
                self.file_overlays,
                self.tmp_backing,
                entry,
            )
        {
            return Some(PathMapping::SamePath);
        }
        None
    }
}

/// Whether `entry` is masked by a tmpfs overlay (and therefore unreadable
/// even when the host rootfs is mounted read-only).
///
/// Explicit [`tmpfs_overlays`] always win.
/// When `/tmp` itself is backed by tmpfs, any path under `/tmp` (except
/// a bind-mounted host directory) counts as hidden.
///
/// [`tmpfs_overlays`]: SandboxLayout::tmpfs_overlays
pub(crate) fn path_hidden_by_overlay(
    tmpfs_overlays: &[Box<Path>],
    file_overlays: &[FileOverlay],
    tmp_backing: &TmpBacking,
    entry: &Path,
) -> bool {
    // Explicit overlays (e.g. /home) always shadow the host.
    if tmpfs_overlays
        .iter()
        .any(|overlay| entry.starts_with(overlay))
    {
        return true;
    }
    // File overlays (e.g. /etc/shadow) mask the exact file.
    if file_overlays
        .iter()
        .any(|overlay| *entry == *overlay.dest())
    {
        return true;
    }
    match tmp_backing {
        // Pure tmpfs: nothing under /tmp comes from the host.
        TmpBacking::Tmpfs => entry.starts_with(Path::new("/tmp")),
        // Bind-mounted host dir: that subdir is real, the rest of /tmp is tmpfs.
        TmpBacking::BindHost(host_dir) => {
            entry.starts_with(Path::new("/tmp")) && !entry.starts_with(host_dir)
        }
    }
}

/// Maps a sandbox prefix and relative path into a sandbox path.
pub(crate) fn join_mapped_path<'a>(base: &'a Path, relative: &Path) -> Cow<'a, Path> {
    if relative.as_os_str().is_empty() {
        Cow::Borrowed(base)
    } else {
        let mut joined =
            PathBuf::with_capacity(base.as_os_str().len() + relative.as_os_str().len() + 1);
        joined.push(base);
        joined.push(relative);
        Cow::Owned(joined)
    }
}

fn map_prefix<'config, 'path>(
    entry: &'path Path,
    host_prefix: &Path,
    dest_prefix: &'config Path,
) -> Option<PathMapping<'config, 'path>> {
    let relative = entry.strip_prefix(host_prefix).ok()?;
    if host_prefix == dest_prefix {
        Some(PathMapping::SamePath)
    } else {
        Some(PathMapping::Remap {
            dest_prefix,
            relative,
        })
    }
}
