//! Shared validation helpers for bubblewrap profiles.
//!
//! Path, symlink, environment variable, and backing-store checks used
//! during profile construction.
//!
//! # Validation
//!
//! Every validator returns [`Result<(), LinuxBwrapError>`] with
//! [`LinuxBwrapError::InvalidPath`] on failure. Validators are designed
//! to be called early and fail fast before a profile is assembled.
//!
//! # Public API
//!
//! - **Directory & path checks**: [`validate_absolute_path`],
//!   [`validate_directory_path`], [`validate_optional_directory_path`],
//!   [`validate_existing_path`], [`validate_mount_paths`]
//! - **Overlay checks**: [`validate_tmpfs_overlays`], [`validate_file_overlays`]
//! - **Symlink checks**: [`validate_symlinks`]
//! - **Environment variable checks**: [`validate_env_vars`]
//! - **Tmp backing checks**: [`validate_tmp_backing`]
//! - **Cache setup**: [`ensure_cache_root_subdirs`]

use super::types::{EnvVar, FileOverlay, Symlink, TmpBacking};
use crate::LinuxBwrapError;
use std::fs;
use std::path::Path;

/// Creates `xdg-cache` and `xdg-state` subdirectories under `cache_root`.
///
/// No-op when `mount_cache_root` is false.
///
/// # Errors
///
/// Returns [`LinuxBwrapError::InvalidPath`] if `cache_root` is not absolute
/// or a subdirectory cannot be created.
pub(crate) fn ensure_cache_root_subdirs(
    mount_cache_root: bool,
    cache_root: &Path,
) -> Result<(), LinuxBwrapError> {
    if !mount_cache_root {
        return Ok(());
    }

    validate_absolute_path(cache_root, "cache root host path")?;
    for subdir in ["xdg-cache", "xdg-state"] {
        let path = cache_root.join(subdir);
        fs::create_dir_all(&path).map_err(|err| {
            LinuxBwrapError::InvalidPath(format!(
                "failed to create cache root subdir {}: {err}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

/// Validates that `path` is absolute.
pub(crate) fn validate_absolute_path(path: &Path, label: &str) -> Result<(), LinuxBwrapError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(LinuxBwrapError::InvalidPath(format!(
            "{label} must be an absolute path: {}",
            path.display()
        )))
    }
}

/// Validates that an optional directory path is absolute, exists, and is a directory.
pub(crate) fn validate_optional_directory_path(
    path: Option<&Path>,
    label: &str,
) -> Result<(), LinuxBwrapError> {
    match path {
        Some(path) => validate_directory_path(path, label),
        None => Ok(()),
    }
}

/// Validates that `path` is an absolute existing directory.
pub(crate) fn validate_directory_path(path: &Path, label: &str) -> Result<(), LinuxBwrapError> {
    validate_absolute_path(path, label)?;
    let metadata = fs::metadata(path).map_err(|_| {
        LinuxBwrapError::InvalidPath(format!("{label} does not exist: {}", path.display()))
    })?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(LinuxBwrapError::InvalidPath(format!(
            "{label} is not a directory: {}",
            path.display()
        )))
    }
}

/// Validates that `path` is an absolute existing path.
pub(crate) fn validate_existing_path(path: &Path, label: &str) -> Result<(), LinuxBwrapError> {
    validate_absolute_path(path, label)?;
    fs::metadata(path).map_err(|_| {
        LinuxBwrapError::InvalidPath(format!("{label} does not exist: {}", path.display()))
    })?;
    Ok(())
}

/// Validates mount source paths.
pub(crate) fn validate_mount_paths(
    mounts: &[Box<Path>],
    label: &str,
) -> Result<(), LinuxBwrapError> {
    for mount in mounts {
        validate_existing_path(mount, label)?;
    }
    Ok(())
}

/// Validates tmpfs overlay destinations.
pub(crate) fn validate_tmpfs_overlays(overlays: &[Box<Path>]) -> Result<(), LinuxBwrapError> {
    for overlay in overlays {
        validate_absolute_path(overlay, "tmpfs overlay path")?;
    }
    Ok(())
}

/// Validates file overlay entries.
///
/// The source must be an absolute path that exists on the host. The destination
/// must be an absolute path.
pub(crate) fn validate_file_overlays(overlays: &[FileOverlay]) -> Result<(), LinuxBwrapError> {
    for overlay in overlays {
        validate_existing_path(overlay.source(), "file overlay source")?;
        validate_absolute_path(overlay.dest(), "file overlay destination")?;
    }
    Ok(())
}

/// Checks that every symlink has a non-empty target and an absolute link path.
///
/// # Errors
///
/// Returns [`LinuxBwrapError::InvalidPath`] for empty targets or non-absolute
/// link paths.
pub(crate) fn validate_symlinks(symlinks: &[Symlink]) -> Result<(), LinuxBwrapError> {
    for symlink in symlinks {
        if symlink.target().is_empty() {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "compat symlink target must not be empty: {}",
                symlink.link_path().display()
            )));
        }
        validate_absolute_path(symlink.link_path(), "compat symlink path")?;
    }
    Ok(())
}

/// Checks that variable names are non-empty, contain no `=`, and neither
/// names nor values contain NUL bytes.
///
/// NUL bytes are rejected because environment variables are stored as C strings
/// in the kernel's `environ` array - a NUL would silently truncate the string
/// at that point.
///
/// # Errors
///
/// Returns [`LinuxBwrapError::InvalidPath`] for the first invalid variable found.
pub(crate) fn validate_env_vars(vars: &[EnvVar], label: &str) -> Result<(), LinuxBwrapError> {
    for var in vars {
        if var.name().is_empty() {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "{label} environment variable name must not be empty"
            )));
        }
        if var.name().contains('=') {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "{label} environment variable name must not contain '=': {}",
                var.name()
            )));
        }
        if var.name().contains('\0') {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "{label} environment variable name must not contain NUL: {}",
                var.name()
            )));
        }
        if var.value().contains('\0') {
            return Err(LinuxBwrapError::InvalidPath(format!(
                "{label} environment variable value must not contain NUL: {}",
                var.name()
            )));
        }
    }
    Ok(())
}

/// Validates that bind-backed `/tmp` targets an existing directory other than
/// the host `/tmp` itself. [`TmpBacking::Tmpfs`] always passes.
///
/// # Errors
///
/// Returns [`LinuxBwrapError::InvalidPath`] if the host directory does not
/// exist, is not a directory, or is exactly `/tmp`.
pub(crate) fn validate_tmp_backing(tmp_backing: &TmpBacking) -> Result<(), LinuxBwrapError> {
    match tmp_backing {
        TmpBacking::Tmpfs => Ok(()),
        TmpBacking::BindHost(host_dir) => {
            validate_directory_path(host_dir, "sandbox tmp host directory")?;
            if host_dir.as_ref() == Path::new("/tmp") {
                return Err(LinuxBwrapError::InvalidPath(
                    "sandbox tmp host directory must not be /tmp; \
                     use a dedicated directory to avoid sharing state \
                     with the host and other sandboxes"
                        .to_string(),
                ));
            }
            Ok(())
        }
    }
}
