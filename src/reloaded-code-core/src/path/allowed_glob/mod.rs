//! Glob-aware workspace-rooted path resolver implementation.
//!
//! Provides [`AllowedGlobResolver`] which restricts path access to a workspace
//! root directory with glob pattern filtering.
//!
//! # Resolution algorithm
//!
//! The entry point (`PathResolver::resolve`) rejects paths that lexically
//! escape the workspace root, then joins relative paths with the workspace root
//! and dispatches to `resolve_candidate`.
//!
//! ## `resolve_candidate`
//!
//! Three resolution tiers, cheapest first:
//!
//! 1. **Canonicalize** - if the file exists, resolve symlinks and normalize.
//!    Accept if the result stays inside the workspace root and passes policy.
//! 2. **New-file fast path** - if only the file is new but its parent
//!    directory exists, canonicalize the parent and join the filename.
//!    Accept if the result stays inside the workspace root and passes policy.
//! 3. **Soft canonicalize** - for paths where even the parent doesn't exist,
//!    resolve as far as possible. Accept if the result stays inside the
//!    workspace root and passes policy.
//!
//! After each resolution tier, check glob policy against the full canonicalized
//! absolute path. If policy denies, reject immediately.
//!
//! If no tier succeeds or policy denies the path, reject with
//! "not within allowed directories".

pub mod normalize;
mod policy;

use super::{path_analysis, resolve_new_file_fast, PathResolver};
use crate::context::PathMode;
use crate::error::{ToolError, ToolResult};
use normalize::{expand_shell, normalize_path};
use soft_canonicalize::soft_canonicalize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use policy::{GlobPolicy, GlobPolicyBuilder, RuleAction};

/// Path resolver that restricts access to a workspace root with glob pattern filtering.
///
/// This is the agent-facing resolver, used when file permissions are configured
/// via frontmatter. For code-level multi-directory containment without glob
/// filtering, use [`super::AllowedPathResolver`].
///
/// # Path Semantics
///
/// - **Absolute paths**: Canonicalized and checked against the policy directly.
/// - **Relative paths**: Joined with the workspace root before canonicalization.
/// - **Policy matching**: Last-match-wins against the full canonicalized absolute path.
/// - **Unmatched paths**: Denied.
#[derive(Debug, Clone)]
pub struct AllowedGlobResolver {
    /// Canonicalized workspace root directory.
    workspace_root: Arc<Path>,
    /// Optional glob policy matching against absolute paths.
    policy: Option<Arc<GlobPolicy>>,
}

impl AllowedGlobResolver {
    /// Creates a new resolver with the given workspace root directory.
    ///
    /// The directory is canonicalized during construction. Shell patterns (`~/`,
    /// `$HOME/`, etc.) are expanded before resolution.
    ///
    /// # Arguments
    ///
    /// - `workspace_root`: Path to the workspace root directory.
    ///
    /// # Returns
    ///
    /// A new [`AllowedGlobResolver`] with no glob policy attached.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if:
    /// - The expanded path is not an existing directory.
    /// - The path cannot be canonicalized.
    pub fn new(workspace_root: impl AsRef<Path>) -> ToolResult<Self> {
        let path = workspace_root.as_ref();
        // Expand shell patterns like ~/, $HOME/, etc.
        let expanded = expand_shell(&path.to_string_lossy())?;
        // Verify the path points to an existing directory.
        if !expanded.is_dir() {
            return Err(ToolError::InvalidPath(format!(
                "failed to resolve workspace root '{}': path is not an existing directory",
                path.display()
            )));
        }
        // Canonicalize to resolve symlinks and normalize the path.
        let canonicalized = soft_canonicalize(&expanded).map_err(|e| {
            ToolError::InvalidPath(format!(
                "failed to resolve workspace root '{}': {}",
                path.display(),
                e
            ))
        })?;
        Ok(Self {
            workspace_root: Arc::from(canonicalized.into_boxed_path()),
            policy: None,
        })
    }

    /// Creates a resolver from an already-canonicalized workspace root.
    ///
    /// Skips shell expansion and canonicalization — use only when the path
    /// is already fully resolved.
    ///
    /// # Arguments
    ///
    /// - `workspace_root`: An already-canonicalized absolute path to the workspace root.
    ///
    /// # Returns
    ///
    /// A new [`AllowedGlobResolver`] with no glob policy attached.
    pub fn from_canonical(workspace_root: impl AsRef<Path>) -> Self {
        debug_assert!(
            workspace_root.as_ref().is_absolute(),
            "from_canonical expects an absolute workspace_root"
        );
        Self {
            workspace_root: Arc::from(workspace_root.as_ref()),
            policy: None,
        }
    }

    /// Sets the glob policy for this resolver.
    ///
    /// # Arguments
    ///
    /// - `policy`: The [`GlobPolicy`] to apply during path resolution.
    ///
    /// # Returns
    ///
    /// `self` with the policy attached, for method chaining.
    pub fn with_policy(mut self, policy: GlobPolicy) -> Self {
        self.policy = Some(Arc::new(policy));
        self
    }

    /// Returns the canonicalized workspace root directory.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Returns the current glob policy, if any.
    pub fn policy(&self) -> Option<&GlobPolicy> {
        self.policy.as_deref()
    }
}

impl PathResolver for AllowedGlobResolver {
    /// Fast per-entry check: is this absolute path allowed?
    ///
    /// Checks `starts_with(workspace_root)` and, if a policy is configured,
    /// normalizes the path and checks against the glob policy.
    fn is_path_allowed(&self, path: &Path) -> bool {
        if !path.starts_with(&self.workspace_root) {
            return false;
        }
        if let Some(policy) = &self.policy {
            let normalized = normalize_path(path);
            policy.is_allowed(normalized.as_ref())
        } else {
            true
        }
    }

    fn path_mode(&self) -> PathMode {
        PathMode::Allowed
    }

    /// Resolves a path against the workspace root and checks glob policy.
    ///
    /// Rejects paths that lexically escape the workspace root, then resolves
    /// the candidate through canonicalization tiers and policy checks.
    ///
    /// # Arguments
    ///
    /// - `path`: A relative or absolute path string to resolve.
    ///
    /// # Returns
    ///
    /// The canonicalized absolute [`PathBuf`] if the path is within the
    /// workspace root and allowed by policy.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if:
    /// - The path lexically escapes the workspace root.
    /// - The resolved path falls outside the workspace root.
    /// - The glob policy denies the resolved path.
    /// - No resolution tier succeeds.
    fn resolve(&self, path: &str) -> ToolResult<PathBuf> {
        let input_path = Path::new(path);

        let analysis = path_analysis(input_path);
        if analysis.escapes {
            return Err(reject(path));
        }

        let candidate = self.workspace_root.join(input_path);
        resolve_candidate(self, path, &candidate)
    }
}

/// Resolve a candidate path against the workspace root with glob policy filtering.
///
/// Three resolution tiers, cheapest first:
///
/// 1. `canonicalize()` for existing files.
/// 2. `resolve_new_file_fast()` for new files in existing dirs.
/// 3. `soft_canonicalize()` fallback for missing parent dirs.
///
/// After each tier, re-check glob policy against the normalized absolute path.
/// Accept the first tier that passes both the containment check and policy.
fn resolve_candidate(
    resolver: &AllowedGlobResolver,
    path: &str,
    candidate: &Path,
) -> ToolResult<PathBuf> {
    // Step 1: canonicalize for existing files - resolves symlinks and normalizes.
    if let Ok(resolved) = candidate.canonicalize() {
        return validate_resolved(resolver, resolved, path);
    }

    // Step 2: fast path for new files in existing directories.
    if let Some(resolved) = resolve_new_file_fast(candidate) {
        return validate_resolved(resolver, resolved, path);
    }

    // Step 3: fallback for paths with missing parent dirs.
    if let Ok(resolved) = soft_canonicalize(candidate) {
        return validate_resolved(resolver, resolved, path);
    }

    Err(reject(path))
}

#[inline]
fn reject(path: &str) -> ToolError {
    ToolError::InvalidPath(format!("path '{}' is not within allowed directories", path))
}

/// Validates a resolved path against workspace containment and policy.
///
/// Delegates to [`AllowedGlobResolver::is_path_allowed`] for a single source
/// of truth on the policy check.
fn validate_resolved(
    resolver: &AllowedGlobResolver,
    resolved: PathBuf,
    path: &str,
) -> ToolResult<PathBuf> {
    if resolved.as_path() == resolver.workspace_root.as_ref() || resolver.is_path_allowed(&resolved)
    {
        Ok(resolved)
    } else {
        Err(reject(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use soft_canonicalize::soft_canonicalize;
    use std::error::Error;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_dir() -> Result<TempDir, Box<dyn Error>> {
        let dir = TempDir::new()?;
        fs::create_dir_all(dir.path().join("src"))?;
        fs::create_dir_all(dir.path().join("target/debug"))?;
        fs::write(dir.path().join("src/lib.rs"), "content")?;
        fs::write(dir.path().join("src/main.rs"), "content")?;
        fs::write(dir.path().join("Cargo.toml"), "content")?;
        fs::write(dir.path().join("target/debug/app"), "binary")?;
        Ok(dir)
    }

    fn resolver_with_policy(
        dir: &TempDir,
        pattern: &str,
    ) -> Result<AllowedGlobResolver, Box<dyn Error>> {
        let root = soft_canonicalize(dir.path())?;
        let policy = GlobPolicy::builder_with_base(&root)?
            .allow(pattern)?
            .build()?;
        Ok(AllowedGlobResolver::new(dir.path())?.with_policy(policy))
    }

    fn setup_src_globstar_dir() -> Result<TempDir, Box<dyn Error>> {
        let dir = TempDir::new()?;
        fs::create_dir_all(dir.path().join("src/deep/nested"))?;
        fs::write(dir.path().join("src/deep/nested/mod.rs"), "content")?;
        fs::write(dir.path().join("src/other.rs"), "content")?;
        fs::write(dir.path().join("src/main.rs"), "content")?;
        fs::write(dir.path().join("src/lib.rs"), "content")?;
        Ok(dir)
    }

    #[test]
    fn constructs_with_valid_directory_and_stores_resolved() -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolved = soft_canonicalize(dir.path())?;
        let resolver = AllowedGlobResolver::new(dir.path())?;
        assert_eq!(resolver.workspace_root(), resolved);
        Ok(())
    }

    #[test]
    fn fails_to_construct_with_nonexistent_directory() {
        let result = AllowedGlobResolver::new(PathBuf::from("/nonexistent/path/xyz"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("failed to resolve workspace root"));
    }

    #[test]
    fn from_canonical_skips_validation() {
        let fake_abs = if cfg!(windows) {
            PathBuf::from("C:\\nonexistent\\path")
        } else {
            PathBuf::from("/nonexistent/path")
        };
        let resolver = AllowedGlobResolver::from_canonical(&fake_abs);
        assert_eq!(resolver.workspace_root(), fake_abs);
    }

    #[test]
    fn resolves_existing_file_within_workspace_root() -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;

        // lib.rs exists from setup_test_dir call, should pass.
        let result = resolver.resolve("src/lib.rs");
        assert!(result.is_ok());
        assert!(result?.ends_with("lib.rs"));
        Ok(())
    }

    #[test]
    fn resolves_new_file_within_workspace_root() -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;

        // new_file.rs does not exist initially but is in allowed directory, should pass.
        let result = resolver.resolve("src/new_file.rs");
        assert!(result.is_ok());
        assert!(result?.ends_with("new_file.rs"));
        Ok(())
    }

    #[test]
    fn resolves_new_file_when_intermediate_directories_do_not_exist() -> Result<(), Box<dyn Error>>
    {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;

        // write targets should still resolve when some parent directories do not exist yet.
        let result = resolver.resolve("src/new_dir/nested/new_file.rs");
        assert!(result.is_ok());
        assert!(result?.ends_with("src/new_dir/nested/new_file.rs"));
        Ok(())
    }

    #[test]
    fn resolves_new_file_with_missing_directories_under_matching_policy(
    ) -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = resolver_with_policy(&dir, "src/**/*.rs")?;

        let result = resolver.resolve("src/new_dir/nested/new_file.rs");
        assert!(result.is_ok());
        assert!(result?.ends_with("src/new_dir/nested/new_file.rs"));
        Ok(())
    }

    #[test]
    fn resolves_existing_file_through_missing_directory_parent_traversal(
    ) -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;

        let result = resolver.resolve("src/new_dir/../../Cargo.toml");
        assert!(result.is_ok());
        assert!(result?.ends_with("Cargo.toml"));
        Ok(())
    }

    #[rstest]
    #[case::parent_traversal("../../../etc/passwd")]
    #[case::nested_traversal("src/../../../etc/passwd")]
    #[case::simple_parent("../Cargo.toml")]
    fn rejects_path_traversal_attempts(#[case] input: &str) -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;

        // Different traversal shapes should all be rejected the same way.
        let result = resolver.resolve(input);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlink_escape_attempt() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        // We don't allow escapes via symlinks.
        // The resolver should catch/reject the path.
        let dir = setup_test_dir()?;
        let escape_target = TempDir::new()?;
        fs::write(escape_target.path().join("secret.txt"), "secret")?;

        let symlink_path = dir.path().join("escape_link");
        symlink(escape_target.path(), &symlink_path)?;

        let resolver = AllowedGlobResolver::new(dir.path())?;

        let result = resolver.resolve("escape_link/secret.txt");
        assert!(result.is_err(), "symlink escape should be blocked");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
        Ok(())
    }

    /// Regression test: symlink under allowed dir pointing to denied dir
    /// must be blocked after canonicalization re-checks policy.
    #[test]
    #[cfg(unix)]
    fn rejects_symlink_policy_bypass_attempt() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::symlink;

        let dir = setup_test_dir()?;
        fs::create_dir_all(dir.path().join("src"))?;
        fs::create_dir_all(dir.path().join("target"))?;
        fs::write(dir.path().join("target/app"), "binary")?;

        let symlink_path = dir.path().join("src/link");
        symlink(dir.path().join("target"), &symlink_path)?;

        let root = soft_canonicalize(dir.path())?;
        let policy = GlobPolicy::builder_with_base(&root)?
            .allow("src/**")?
            .deny("target/**")?
            .build()?;

        let resolver = AllowedGlobResolver::new(dir.path())?.with_policy(policy);

        let result = resolver.resolve("src/link/app");
        assert!(
            result.is_err(),
            "symlink policy bypass should be blocked: 'src/link/app' resolves to 'target/app' which should be denied"
        );
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
        Ok(())
    }

    #[test]
    fn rejects_absolute_path_outside_workspace_root() -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;
        let external = std::env::temp_dir().join("some-external-path.txt");
        let result = resolver.resolve(external.to_str().ok_or("invalid path")?);
        let err = result.expect_err("external path should be rejected");
        assert!(err.to_string().contains("not within allowed"));
        Ok(())
    }

    /// In-base paths denied by absolute policy are rejected.
    #[rstest]
    #[case::cargo_toml("Cargo.toml")]
    #[case::target_binary("target/debug/app")]
    fn rejects_in_base_path_denied_by_absolute_policy(
        #[case] path: &str,
    ) -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = resolver_with_policy(&dir, "src/**")?;
        let full_path = dir.path().join(path);
        let result = resolver.resolve(full_path.to_str().ok_or("invalid path")?);
        assert!(
            result.is_err(),
            "'{}' is inside workspace root but denied by absolute src/** policy",
            path
        );
        Ok(())
    }

    #[rstest]
    #[case::src_lib_should_be_allowed("src/lib.rs", true)]
    #[case::cargo_toml_should_be_denied("Cargo.toml", false)]
    #[case::target_binary_should_be_denied("target/debug/app", false)]
    fn resolver_with_src_policy_should_allow_only_matching_relative_paths(
        #[case] input: &str,
        #[case] expected_ok: bool,
    ) -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = resolver_with_policy(&dir, "src/**")?;

        // One policy, multiple paths: only matching relative paths should resolve.
        let result = resolver.resolve(input);
        assert!(
            result.is_ok() == expected_ok,
            "path '{input}' should {}match absolute src/**",
            if expected_ok { "" } else { "not " }
        );
        Ok(())
    }

    #[rstest]
    #[case::new_rs_file_should_be_allowed("new_file.rs", true)]
    #[case::new_txt_file_should_be_denied("new_file.txt", false)]
    fn resolver_should_check_policy_for_new_paths(
        #[case] input: &str,
        #[case] expected_ok: bool,
    ) -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = resolver_with_policy(&dir, "*.rs")?;

        // New paths are checked against policy before the file exists.
        let result = resolver.resolve(input);
        assert!(
            result.is_ok() == expected_ok,
            "path '{input}' should {}match '*.rs'",
            if expected_ok { "" } else { "not " }
        );
        Ok(())
    }

    #[test]
    fn returns_resolved_path_without_dotdots() -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = AllowedGlobResolver::new(dir.path())?;

        let resolved = resolver.resolve("src/../Cargo.toml")?;
        assert!(
            !resolved.to_string_lossy().contains(".."),
            "resolved path should not contain '..'"
        );
        Ok(())
    }

    #[rstest]
    #[case::src_lib_should_be_allowed("src/lib.rs", true)]
    #[case::src_main_should_be_allowed("src/main.rs", true)]
    #[case::nested_mod_should_be_allowed("src/deep/nested/mod.rs", true)]
    #[case::src_other_should_be_allowed("src/other.rs", true)]
    #[case::cargo_toml_should_be_denied("Cargo.toml", false)]
    #[case::target_binary_should_be_denied("target/debug/app", false)]
    fn resolver_with_src_globstar_policy_should_match_expected_paths(
        #[case] input: &str,
        #[case] expected_ok: bool,
    ) -> Result<(), Box<dyn Error>> {
        let dir = setup_src_globstar_dir()?;
        let resolver = resolver_with_policy(&dir, "src/**/*.rs")?;

        // Verify globstar behavior across shallow, deep, and denied paths.
        let result = resolver.resolve(input);
        assert!(
            result.is_ok() == expected_ok,
            "path '{input}' should {}match 'src/**/*.rs'",
            if expected_ok { "" } else { "not " }
        );
        Ok(())
    }

    #[test]
    fn canonicalizes_path_before_policy_check() -> Result<(), Box<dyn Error>> {
        let dir = setup_test_dir()?;
        let resolver = resolver_with_policy(&dir, "src/**")?;

        let input = dir
            .path()
            .join("src")
            .join("new_dir")
            .join("..")
            .join("lib.rs");
        let result = resolver.resolve(&input.to_string_lossy());
        assert!(
            result.is_ok(),
            "canonicalized path should match absolute policy"
        );
        let resolved = result?;
        assert!(!resolved.to_string_lossy().contains(".."));
        Ok(())
    }
}
