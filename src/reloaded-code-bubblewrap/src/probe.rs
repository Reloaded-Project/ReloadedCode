//! Checks whether `bwrap` can run.
//!
//! Most callers should use [`crate::profile::Availability::detect`].

use crate::path_util::normalize_path;
use crate::{Availability, LinuxBwrapError, Preset};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::OnceLock;

/// A no-op shell command used as the probe payload.
const PROBE_COMMAND: &str = ":";
/// Sentinel argument appended to the probe command to distinguish its logs.
pub(crate) const PROBE_ARG0: &str = "__reloaded_code_bwrap_probe__";
/// Absolute paths checked when `PATH` lookups for `bash`/`sh` yield nothing.
const SHELL_CANDIDATES: &[&str] = &[
    "/run/current-system/sw/bin/bash",
    "/nix/var/nix/profiles/default/bin/bash",
    "/usr/bin/bash",
    "/bin/bash",
    "/run/current-system/sw/bin/sh",
    "/nix/var/nix/profiles/default/bin/sh",
    "/usr/bin/sh",
    "/bin/sh",
];

/// Outcome of probing the host for a working `bwrap` binary.
#[derive(Clone, Debug)]
enum LinuxBwrapBackend {
    /// `bwrap` was found and successfully created a sandbox.
    Available { bwrap: Arc<Path> },
    /// No `bwrap` binary exists on `PATH`.
    MissingBinary { reason: Box<str> },
    /// `bwrap` exists but the environment cannot run sandboxes (e.g. missing namespaces).
    Unusable { reason: Box<str> },
}

/// Returns whether `bwrap` is usable on this host.
///
/// Results are cached per `PATH` value within the process lifetime.
pub(crate) fn probe_availability() -> Availability {
    match probe_backend() {
        LinuxBwrapBackend::Available { .. } => Availability::Available,
        LinuxBwrapBackend::MissingBinary { reason } | LinuxBwrapBackend::Unusable { reason } => {
            Availability::Unavailable { reason }
        }
    }
}

/// Returns the path to `bwrap` or an error explaining why it cannot be used.
///
/// If `availability` already indicates unavailability, returns early without
/// probing again. Otherwise re-checks the host and returns an [`Arc<Path>`] on
/// success or a [`LinuxBwrapError::Execution`] on failure.
///
/// # Errors
/// - Returns [`LinuxBwrapError::Execution`] when the provided `availability` already
///   indicates unavailability (via [`Availability::reason`]).
/// - Returns [`LinuxBwrapError::Execution`] when the `bwrap` binary cannot be found on `PATH`.
/// - Returns [`LinuxBwrapError::Execution`] when `bwrap` exists but the current environment
///   cannot create sandboxes (e.g., missing user namespace capabilities).
pub(crate) fn resolve_backend_or_error_for(
    preset: Option<Preset>,
    availability: &Availability,
) -> Result<Arc<Path>, LinuxBwrapError> {
    if let Some(reason) = availability.reason() {
        return Err(LinuxBwrapError::Execution(format!(
            "linux sandbox profile {} is unavailable: {}",
            profile_name(preset),
            reason,
        )));
    }

    match probe_backend() {
        LinuxBwrapBackend::Available { bwrap } => Ok(bwrap),
        LinuxBwrapBackend::MissingBinary { reason } => Err(LinuxBwrapError::Execution(format!(
            "linux sandbox profile {} requires bubblewrap (`bwrap`), but no usable binary was found: {}",
            profile_name(preset),
            reason,
        ))),
        LinuxBwrapBackend::Unusable { reason } => Err(LinuxBwrapError::Execution(format!(
            "linux sandbox profile {} requires bubblewrap (`bwrap`), but the current environment cannot create a sandbox: {}",
            profile_name(preset),
            reason,
        ))),
    }
}

fn profile_name(preset: Option<Preset>) -> &'static str {
    match preset {
        Some(Preset::PublicBot) => "PublicBot",
        Some(Preset::TrustedMaintenance) => "TrustedMaintenance",
        None => "Custom",
    }
}

#[inline]
fn find_binary_on_path_in(name: &str, path: Option<&OsStr>) -> Option<Box<Path>> {
    let path = path?;
    for dir in env::split_paths(path) {
        if !dir.is_absolute() || dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.into_boxed_path());
        }
    }
    None
}

/// Returns the first shell binary for which `classify` returns [`Some`],
/// checking `PATH` first then the hardcoded [`SHELL_CANDIDATES`].
///
/// On success the host path and the classifier's return value are yielded
/// together so the caller need not re-classify.
pub(crate) fn first_shell_candidate_with<F, R>(mut classify: F) -> Option<(Box<Path>, R)>
where
    F: FnMut(&Path) -> Option<R>,
{
    first_shell_candidate_with_in(env::var_os("PATH").as_deref(), &mut classify)
}

fn first_shell_candidate_with_in<F, R>(
    env_path: Option<&OsStr>,
    classify: &mut F,
) -> Option<(Box<Path>, R)>
where
    F: FnMut(&Path) -> Option<R>,
{
    let mut seen = HashSet::with_capacity(10);

    for name in ["bash", "sh"] {
        if let Some(shell_path) = find_binary_on_path_in(name, env_path) {
            if let Some(result) = classify_shell_candidate(classify, &mut seen, shell_path) {
                return Some(result);
            }
        }
    }

    for candidate in SHELL_CANDIDATES {
        let candidate_path = PathBuf::from(candidate);
        if candidate_path.is_file() {
            if let Some(result) =
                classify_shell_candidate(classify, &mut seen, candidate_path.into_boxed_path())
            {
                return Some(result);
            }
        }
    }

    None
}

#[inline]
fn classify_shell_candidate<F, R>(
    classify: &mut F,
    seen: &mut HashSet<Box<Path>>,
    path: Box<Path>,
) -> Option<(Box<Path>, R)>
where
    F: FnMut(&Path) -> Option<R>,
{
    let path = normalize_path(path.as_ref());
    if !seen.insert(path.clone()) {
        return None;
    }
    classify(path.as_ref()).map(|result| (path, result))
}

#[inline]
fn resolve_host_shell_in(path: Option<&OsStr>) -> Option<Box<Path>> {
    first_shell_candidate_with_in(path, &mut |_| Some(())).map(|(path, _)| path)
}

fn probe_backend() -> LinuxBwrapBackend {
    // Cache keyed on PATH: a changed PATH invalidates the result.
    #[allow(clippy::type_complexity)]
    static CACHE: OnceLock<RwLock<Option<(Option<OsString>, LinuxBwrapBackend)>>> = OnceLock::new();

    let path = env::var_os("PATH");
    let cache = CACHE.get_or_init(|| RwLock::new(None));

    {
        let cache = cache.read();
        if let Some((cached_path, cached_backend)) = cache.as_ref() {
            if cached_path == &path {
                return cached_backend.clone();
            }
        }
    }

    let backend = probe_backend_uncached(path.as_deref());
    *cache.write() = Some((path, backend.clone()));
    backend
}

/// Checks `bwrap` without using the cache.
///
/// The probe binds the host root read-only and runs a tiny shell command. That
/// checks both namespace support and shell visibility on FHS and Nix systems.
fn probe_backend_uncached(path: Option<&OsStr>) -> LinuxBwrapBackend {
    let Some(bwrap) = find_binary_on_path_in("bwrap", path) else {
        return LinuxBwrapBackend::MissingBinary {
            reason: Box::from("`bwrap` was not found on PATH"),
        };
    };

    let Some(shell) = resolve_host_shell_in(path) else {
        return LinuxBwrapBackend::Unusable {
            reason: Box::from("no usable host shell (`bash` or `sh`) was found"),
        };
    };

    // Verify that bwrap can actually create namespaces by running a minimal
    // sandbox (host root read-only, no-op command). Finding the binary on PATH
    // is not enough; the process may lack user-namespace capabilities.
    // PROBE_ARG0 appears as $0 so the probe is identifiable in logs/audit.
    let probe = Command::new(bwrap.as_os_str())
        .args([
            "--die-with-parent",
            "--new-session",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--ro-bind",
            "/",
            "/",
            "--",
        ])
        .arg(shell.as_os_str())
        .arg("-c")
        .arg(PROBE_COMMAND)
        .arg(PROBE_ARG0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();

    match probe {
        Ok(output) if output.status.success() => LinuxBwrapBackend::Available {
            bwrap: Arc::from(bwrap),
        },
        Ok(output) => LinuxBwrapBackend::Unusable {
            reason: probe_failure_reason(&output, "bubblewrap probe failed"),
        },
        Err(error) => LinuxBwrapBackend::Unusable {
            reason: format!("failed to execute bubblewrap probe: {error}").into_boxed_str(),
        },
    }
}

/// Extracts a human-readable failure reason from a failed process output.
///
/// Prefers the process stderr; falls back to `fallback` plus the exit status
/// when stderr is empty.
fn probe_failure_reason(output: &Output, fallback: &str) -> Box<str> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        format!("{fallback} (exit status: {})", output.status).into_boxed_str()
    } else {
        trimmed.to_owned().into_boxed_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{replace_path, write_script};
    use crate::Preset;
    use serial_test::serial;
    use tempfile::TempDir;

    /// Searches `PATH` directories for a file named `name` and returns the first match.
    fn find_binary_on_path(name: &str) -> Option<Box<Path>> {
        find_binary_on_path_in(name, env::var_os("PATH").as_deref())
    }

    // These tests swap PATH and exercise a process-wide availability cache, so
    // cases that probe `bwrap` run serially to avoid cross-test contamination.

    // Reports a missing-binary error when `PATH` has no `bwrap`.
    #[test]
    #[serial]
    fn probe_backend_classifies_missing_binary() {
        let temp = TempDir::new().unwrap();

        // Point PATH at an empty directory.
        let _guard = replace_path(temp.path());

        // Run the lookup and capture the error text.
        let result = resolve_backend_or_error_for(Some(Preset::PublicBot), &Availability::Unknown);

        assert!(result.is_err());
        let err_text = format!("{:?}", result.unwrap_err());

        // Check that the error explains the missing binary.
        assert!(
            err_text.contains("bwrap"),
            "error should mention bwrap: {}",
            err_text
        );
        assert!(
            err_text.contains("not found") || err_text.contains("binary"),
            "error should indicate missing binary: {}",
            err_text
        );
    }

    // Reports an unusable-environment error when `bwrap` exists but cannot sandbox.
    #[test]
    #[serial]
    fn probe_backend_classifies_unusable_environment() {
        let temp = TempDir::new().unwrap();

        let error_msg = "bwrap: Cannot create new namespace";
        // Make `bwrap` look installed, then fail the "can it sandbox?" probe.
        let script = format!(
            r#"#!/bin/sh
echo "{}" >&2
exit 1
"#,
            error_msg
        );
        write_script(temp.path(), "bwrap", &script);

        // Run the probe against the fake binary.
        let _guard = replace_path(temp.path());

        let result = resolve_backend_or_error_for(Some(Preset::PublicBot), &Availability::Unknown);

        assert!(result.is_err());
        let err_text = format!("{:?}", result.unwrap_err());

        // Check that the namespace failure reaches callers.
        assert!(
            err_text.contains("bwrap"),
            "error should mention bwrap: {}",
            err_text
        );
        assert!(
            err_text.contains("Cannot create new namespace"),
            "error should preserve namespace error: {}",
            err_text
        );
        assert!(
            !err_text.contains("fallback"),
            "error should not mention fallback: {}",
            err_text
        );
    }

    // Finds `bash` when it is present on the host PATH.
    #[test]
    fn find_binary_on_path_finds_bash() {
        // Look up `bash` on the current PATH.
        let result = find_binary_on_path("bash");

        // Keep this tolerant because some minimal environments expose only `sh`.
        if let Some(path) = result {
            assert!(path.ends_with("bash"));
        }
    }

    // Returns `None` for a binary name that does not exist.
    #[test]
    fn find_binary_on_path_returns_none_for_nonexistent() {
        // Query a name that should never resolve.
        let result = find_binary_on_path("definitely_not_a_real_binary_12345");
        assert!(result.is_none());
    }

    // Reuses a known unavailable reason instead of probing again.
    #[test]
    fn unavailable_config_returns_early_error() {
        // Start from an availability result that already failed.
        let result = resolve_backend_or_error_for(
            Some(Preset::PublicBot),
            &Availability::unavailable("test reason"),
        );

        assert!(result.is_err());
        let err_text = format!("{:?}", result.unwrap_err());

        // Check that the original reason is preserved.
        assert!(err_text.contains("test reason"));
        assert!(err_text.contains("unavailable"));
    }

    // Uses the `Custom` profile name when no preset is selected.
    #[test]
    #[serial]
    fn custom_builder_profile_returns_execution_error_not_panic() {
        let temp = TempDir::new().unwrap();

        // Remove `bwrap` from PATH and probe the custom profile.
        let _guard = replace_path(temp.path());
        let result = resolve_backend_or_error_for(None, &Availability::Unknown);

        assert!(result.is_err());
        let err_text = format!("{:?}", result.unwrap_err());

        // Check that the error stays user-facing.
        assert!(
            err_text.contains("Custom"),
            "error should mention Custom profile: {}",
            err_text
        );
        assert!(
            err_text.contains("bwrap"),
            "error should mention bwrap: {}",
            err_text
        );
    }
}
