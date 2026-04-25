//! Workspace root resolution.
//!
//! Provides [`resolve_workspace_root`] for determining the workspace root
//! directory. Re-exported at the crate root for convenience.
//!
//! Call once at construction time and cache the result.

use std::path::PathBuf;

/// Resolves the workspace root directory.
///
/// Prefers the git repository root (closest ancestor containing `.git`).
/// Falls back to the current working directory when not inside a git repo.
///
/// Call once at construction time and cache the result. Do not call per-request.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the current working directory cannot be
/// determined (e.g. it has been deleted or permissions prevent access).
///
/// # Returns
///
/// `Ok(absolute_path)` on success - an absolute [`PathBuf`] pointing at the
/// workspace root.
pub fn resolve_workspace_root() -> Result<PathBuf, std::io::Error> {
    let cwd = std::env::current_dir()?;
    let mut candidate = cwd.as_path();
    loop {
        if candidate.join(".git").exists() {
            return Ok(candidate.to_path_buf());
        }
        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => break,
        }
    }
    Ok(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use soft_canonicalize::soft_canonicalize;
    use tempfile::TempDir;

    #[test]
    fn resolve_workspace_root_should_return_absolute_path() {
        let root = resolve_workspace_root().unwrap();
        assert!(
            root.is_absolute(),
            "workspace root must be absolute: {:?}",
            root
        );
    }

    #[test]
    fn resolve_workspace_root_should_return_existing_directory() {
        let root = resolve_workspace_root().unwrap();
        assert!(root.is_dir(), "workspace root must exist: {:?}", root);
    }

    #[test]
    fn resolve_workspace_root_should_prefer_git_root_when_in_repo() {
        let root = resolve_workspace_root().unwrap();
        let in_repo = std::env::current_dir()
            .unwrap()
            .ancestors()
            .any(|p| p.join(".git").exists());
        assert!(
            !in_repo || root.join(".git").exists(),
            "inside a git repo, workspace root should contain .git: {:?}",
            root
        );
    }

    #[test]
    #[serial]
    fn resolve_workspace_root_should_fall_back_to_cwd_when_not_in_repo() {
        let temp = TempDir::new().unwrap();

        if temp.path().ancestors().any(|p| p.join(".git").exists()) {
            return;
        }

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();

        let root = resolve_workspace_root().unwrap();
        assert_eq!(
            soft_canonicalize(&root).unwrap(),
            soft_canonicalize(temp.path()).unwrap(),
            "outside git repo, workspace root should equal cwd"
        );

        std::env::set_current_dir(original).unwrap();
    }
}
