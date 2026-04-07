//! Glob-aware allowed directory path resolver implementation.
//!
//! Provides [`AllowedGlobResolver`] which restricts path access to allowed
//! directories with glob pattern filtering.

pub(crate) mod normalize;
mod policy;

use super::{path_analysis, resolve_new_file_fast, PathResolver};
use crate::context::PathMode;
use crate::error::{ToolError, ToolResult};
use crate::permissions::{PermissionAction, Ruleset};
use normalize::expand_shell;
use soft_canonicalize::soft_canonicalize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use policy::{GlobPolicy, GlobPolicyBuilder, RuleAction};

/// Path resolver that restricts access to allowed directories with glob pattern filtering.
///
/// Resolves paths relative to configured base directories, validating they stay within
/// allowed boundaries. Prevents path traversal attacks and applies glob policy filtering.
///
/// # Path Semantics
///
/// - **Slash normalization**: Paths normalized to `/` for consistent cross-platform matching
/// - **Shell expansion (directories only)**: Base directories support `~/`,
///   `$HOME/`, and other shell patterns. Patterns are always relative to base
///   directory without shell expansion.
/// - **Relative matching**: Patterns match relative paths within base
///   directories (e.g., `src/lib.rs`)
/// - **Last-match-wins**: Last matching rule wins, enabling override patterns via reverse
///   iteration for O(k) short-circuit.
///
/// # Security
///
/// Path traversal is prevented by resolving symlinks and normalizing the
/// resolved path, verifying it stays within allowed base directories, and
/// applying glob policy. Patterns match normalized relative paths with
/// last-match-wins semantics. Unmatched paths are denied.
#[derive(Debug, Clone)]
pub struct AllowedGlobResolver {
    /// Allowed base directories.
    base_directories: Arc<[Arc<Path>]>,
    /// Optional glob policy for file filtering.
    policy: Option<Arc<GlobPolicy>>,
    /// Optional permission ruleset for paths outside allowed bases.
    ///
    /// If a path doesn't resolve within any base, the `"external_directory"` key
    /// is checked against the canonicalized path. Only absolute paths are eligible.
    external_permission: Option<Arc<Ruleset>>,
}

impl AllowedGlobResolver {
    /// Creates a new resolver with the given allowed directories.
    ///
    /// Directories are resolved (symlinks expanded, made absolute, and
    /// normalized) during construction. Shell patterns (`~/`, `$HOME/`, `$VAR`,
    /// etc.) are expanded before resolution.
    ///
    /// Returns `ToolError::InvalidPath` if any directory doesn't exist or cannot
    /// be resolved.
    ///
    /// # Example
    ///
    /// ```
    /// use llm_coding_tools_core::path::AllowedGlobResolver;
    /// use std::path::PathBuf;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let directories = vec![PathBuf::from("/home/user/project")];
    /// let resolver = AllowedGlobResolver::new(directories)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(directories: impl IntoIterator<Item = impl AsRef<Path>>) -> ToolResult<Self> {
        let resolved: Result<Arc<[Arc<Path>]>, _> = directories
            .into_iter()
            .map(|p| {
                let path = p.as_ref();
                let expanded = expand_shell(&path.to_string_lossy())?;
                if !expanded.is_dir() {
                    return Err(ToolError::InvalidPath(format!(
                        "failed to resolve base directory '{}': path is not an existing directory",
                        path.display()
                    )));
                }

                soft_canonicalize(&expanded)
                    .map(|pb| Arc::from(pb.into_boxed_path()))
                    .map_err(|e| {
                        ToolError::InvalidPath(format!(
                            "failed to resolve base directory '{}': {}",
                            path.display(),
                            e
                        ))
                    })
            })
            .collect();

        Ok(Self {
            base_directories: resolved?,
            policy: None,
            external_permission: None,
        })
    }

    /// Creates a resolver from already-resolved paths, skipping filesystem
    /// validation.
    ///
    /// A resolved path is absolute, with all symlinks expanded and all `.` and
    /// `..` components normalized. Use [`std::fs::canonicalize`] or
    /// [`std::path::Path::canonicalize`] to resolve paths.
    ///
    /// Caller must ensure paths are resolved. Using non-resolved paths may
    /// allow path traversal attacks.
    pub fn from_canonical(directories: impl IntoIterator<Item = impl AsRef<Path>>) -> Self {
        Self {
            base_directories: directories
                .into_iter()
                .map(|p| Arc::from(p.as_ref()))
                .collect(),
            policy: None,
            external_permission: None,
        }
    }

    /// Sets the glob policy for this resolver.
    ///
    /// Returns self for method chaining.
    pub fn with_policy(mut self, policy: GlobPolicy) -> Self {
        self.policy = Some(Arc::new(policy));
        self
    }

    /// Allows access to paths outside base directories via a permission ruleset.
    ///
    /// Paths that don't resolve within any base are checked against the
    /// `"external_directory"` permission key. Only absolute paths are eligible;
    /// relative paths always fail.
    ///
    /// # Arguments
    /// - `permission` - [`Ruleset`] controlling external directory access.
    ///
    /// # Returns
    /// The modified resolver for chaining. This method always returns `Self` and
    /// is infallible.
    #[must_use]
    pub fn with_external_permission(mut self, permission: Arc<Ruleset>) -> Self {
        self.external_permission = Some(permission);
        self
    }

    /// Returns the allowed base directories.
    pub fn base_directories(&self) -> &[Arc<Path>] {
        &self.base_directories
    }

    /// Returns the current glob policy, if any.
    pub fn policy(&self) -> Option<&GlobPolicy> {
        self.policy.as_deref()
    }
}

impl PathResolver for AllowedGlobResolver {
    const PATH_MODE: PathMode = PathMode::Allowed;

    fn resolve(&self, path: &str) -> ToolResult<PathBuf> {
        let input_path = Path::new(path);
        let policy = self.policy.as_deref();

        let analysis = path_analysis(input_path);
        if analysis.escapes {
            return Err(ToolError::InvalidPath(format!(
                "path '{}' is not within allowed directories",
                path
            )));
        }

        // Optimization: if path has no `.` or `..` components, we can check the glob
        // policy on the input path directly and only fall back to the resolved path
        // check when canonicalization changes what the policy should see.
        let fast_policy_input = if !analysis.has_dots { Some(path) } else { None };

        for base_dir in self.base_directories.iter() {
            // Fast policy check before filesystem operations.
            if let (Some(policy), Some(input)) = (policy, fast_policy_input) {
                if !policy.is_allowed(input) {
                    continue;
                }
            }

            // Relative input joins base_dir; absolute input overrides it.
            let candidate = base_dir.join(input_path);

            // Existing file/dir: canonicalize resolves symlinks and normalizes.
            if let Ok(resolved) = candidate.canonicalize() {
                // Reject if symlink escapes outside base_dir.
                if !resolved.starts_with(base_dir) {
                    continue;
                }

                // Re-check policy after resolution if normalization or symlinks changed
                // the path seen by glob matching.
                if let Some(policy) = policy {
                    let relative_path = resolved.strip_prefix(base_dir).unwrap_or(Path::new(""));
                    let normalized_relative = normalize::normalize_path(relative_path);
                    if fast_policy_input != Some(normalized_relative.as_ref())
                        && !policy.is_allowed(&normalized_relative)
                    {
                        continue;
                    }
                }

                return Ok(resolved);
            }

            // Fast path for new files in existing directories.
            // Canonicalizes parent directory, then joins filename.
            // This avoids soft_canonicalize's expensive walk-up logic.
            if let Some(resolved) = resolve_new_file_fast(&candidate) {
                if !resolved.starts_with(base_dir) {
                    continue;
                }

                // Re-check policy after resolution if normalization or symlinks changed
                // the path seen by glob matching.
                if let Some(policy) = policy {
                    let relative_path = resolved.strip_prefix(base_dir).unwrap_or(Path::new(""));
                    let normalized_relative = normalize::normalize_path(relative_path);
                    if fast_policy_input != Some(normalized_relative.as_ref())
                        && !policy.is_allowed(&normalized_relative)
                    {
                        continue;
                    }
                }

                return Ok(resolved);
            }

            // Fallback for paths where parent doesn't exist.
            if let Ok(target_path) = soft_canonicalize(&candidate) {
                if !target_path.starts_with(base_dir) {
                    continue;
                }

                // Re-check policy after resolution if normalization or symlinks changed
                // the path seen by glob matching.
                if let Some(policy) = policy {
                    let relative_path = target_path.strip_prefix(base_dir).unwrap_or(Path::new(""));
                    let normalized_relative = normalize::normalize_path(relative_path);
                    if fast_policy_input != Some(normalized_relative.as_ref())
                        && !policy.is_allowed(&normalized_relative)
                    {
                        continue;
                    }
                }

                return Ok(target_path);
            }
        }

        // External directory fallback: check "external_directory" permission.
        // Only absolute paths are eligible.
        if input_path.is_absolute() {
            if let Some(perm) = &self.external_permission {
                let canon = soft_canonicalize(input_path).map_err(|_| {
                    ToolError::InvalidPath(format!(
                        "path '{}' is not within allowed directories",
                        path
                    ))
                })?;
                if perm.evaluate("external_directory", &canon.to_string_lossy())
                    == PermissionAction::Allow
                {
                    return Ok(canon);
                }
            }
        }

        Err(ToolError::InvalidPath(format!(
            "path '{}' is not within allowed directories",
            path
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{PermissionAction, Rule, Ruleset};
    use rstest::rstest;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "content").unwrap();
        fs::write(dir.path().join("src/main.rs"), "content").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "content").unwrap();
        fs::write(dir.path().join("target/debug/app"), "binary").unwrap();
        dir
    }

    // Keeps policy-focused tests small and readable.
    fn resolver_with_policy(dir: &TempDir, pattern: &str) -> AllowedGlobResolver {
        let policy = GlobPolicy::builder()
            .allow(pattern)
            .unwrap()
            .build()
            .unwrap();

        AllowedGlobResolver::new(vec![dir.path().to_path_buf()])
            .unwrap()
            .with_policy(policy)
    }

    fn resolver_with_external_rule(
        dir: &TempDir,
        pattern: &str,
        action: PermissionAction,
    ) -> AllowedGlobResolver {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("external_directory", pattern, action).unwrap());
        AllowedGlobResolver::new(vec![dir.path().to_path_buf()])
            .unwrap()
            .with_external_permission(Arc::new(ruleset))
    }

    // Builds a deeper tree for globstar matching cases.
    fn setup_src_globstar_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src/deep/nested")).unwrap();
        fs::write(dir.path().join("src/deep/nested/mod.rs"), "content").unwrap();
        fs::write(dir.path().join("src/other.rs"), "content").unwrap();
        fs::write(dir.path().join("src/main.rs"), "content").unwrap();
        fs::write(dir.path().join("src/lib.rs"), "content").unwrap();
        dir
    }

    #[test]
    fn constructs_with_valid_directories() {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]);
        assert!(resolver.is_ok());
    }

    #[test]
    fn constructs_with_non_resolved_directory_and_stores_resolved() {
        let dir = setup_test_dir();
        let resolved = dir.path().canonicalize().unwrap();

        fs::create_dir_all(dir.path().join("subdir")).unwrap();

        // Build a path that resolves back to the temp dir.
        let non_resolved = dir.path().join("subdir").join("..");

        // Construction should canonicalize before storing the base directory.
        let resolver = AllowedGlobResolver::new(vec![&non_resolved]).unwrap();

        let stored = resolver.base_directories()[0].as_ref();
        assert_eq!(
            stored,
            resolved.as_path(),
            "stored directory should be resolved"
        );

        // The stored path should not retain unresolved components.
        assert!(
            !stored.to_string_lossy().contains("/subdir/../"),
            "stored path should not contain unresolved components"
        );
    }

    #[test]
    fn fails_to_construct_with_nonexistent_directory() {
        let result = AllowedGlobResolver::new(vec![PathBuf::from("/nonexistent/path/xyz")]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("failed to resolve base directory"));
    }

    #[test]
    fn from_canonical_skips_validation() {
        let resolver = AllowedGlobResolver::from_canonical(vec![PathBuf::from("/some/path")]);
        assert_eq!(resolver.base_directories().len(), 1);
    }

    #[test]
    fn resolves_existing_file_within_base_directory() {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        // lib.rs exists from setup_test_dir call, should pass.
        let result = resolver.resolve("src/lib.rs");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("lib.rs"));
    }

    #[test]
    fn resolves_new_file_within_base_directory() {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        // new_file.rs does not exist initially but is in allowed directory, should pass.
        let result = resolver.resolve("src/new_file.rs");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("new_file.rs"));
    }

    #[test]
    fn resolves_new_file_when_intermediate_directories_do_not_exist() {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        // write targets should still resolve when some parent directories do not exist yet.
        let result = resolver.resolve("src/new_dir/nested/new_file.rs");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("src/new_dir/nested/new_file.rs"));
    }

    #[test]
    fn resolves_new_file_with_missing_directories_under_matching_policy() {
        let dir = setup_test_dir();
        let resolver = resolver_with_policy(&dir, "src/**/*.rs");

        let result = resolver.resolve("src/new_dir/nested/new_file.rs");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("src/new_dir/nested/new_file.rs"));
    }

    #[test]
    fn resolves_existing_file_through_missing_directory_parent_traversal() {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("src/new_dir/../../Cargo.toml");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("Cargo.toml"));
    }

    #[rstest]
    #[case::parent_traversal("../../../etc/passwd")]
    #[case::nested_traversal("src/../../../etc/passwd")]
    #[case::simple_parent("../Cargo.toml")]
    fn rejects_path_traversal_attempts(#[case] input: &str) {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        // Different traversal shapes should all be rejected the same way.
        let result = resolver.resolve(input);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
    }

    #[test]
    #[cfg(unix)]
    fn rejects_symlink_escape_attempt() {
        use std::os::unix::fs::symlink;

        // We don't allow escapes via symlinks.
        // The resolver should catch/reject the path.
        let dir = setup_test_dir();
        let escape_target = TempDir::new().unwrap();
        fs::write(escape_target.path().join("secret.txt"), "secret").unwrap();

        let symlink_path = dir.path().join("escape_link");
        symlink(escape_target.path(), &symlink_path).unwrap();

        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let result = resolver.resolve("escape_link/secret.txt");
        assert!(result.is_err(), "symlink escape should be blocked");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
    }

    /// Regression test: symlink under allowed dir pointing to denied dir
    /// must be blocked after canonicalization re-checks policy.
    #[test]
    #[cfg(unix)]
    fn rejects_symlink_policy_bypass_attempt() {
        use std::os::unix::fs::symlink;

        let dir = setup_test_dir();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/app"), "binary").unwrap();

        let symlink_path = dir.path().join("src/link");
        symlink(dir.path().join("target"), &symlink_path).unwrap();

        let policy = GlobPolicy::builder()
            .allow("src/**")
            .unwrap()
            .deny("target/**")
            .unwrap()
            .build()
            .unwrap();

        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()])
            .unwrap()
            .with_policy(policy);

        let result = resolver.resolve("src/link/app");
        assert!(
            result.is_err(),
            "symlink policy bypass should be blocked: 'src/link/app' resolves to 'target/app' which should be denied"
        );
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
    }

    // When multiple base directories are present; we should test all of them
    // in a specified order.
    #[test]
    fn tries_multiple_base_directories() {
        let dir1 = setup_test_dir();
        let dir2 = setup_test_dir();
        fs::write(dir2.path().join("only_in_dir2.txt"), "content").unwrap();

        let resolver =
            AllowedGlobResolver::new(vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()])
                .unwrap();

        let result = resolver.resolve("only_in_dir2.txt");
        assert!(result.is_ok());
    }

    #[rstest]
    #[case::src_lib_should_be_allowed("src/lib.rs", true)]
    #[case::cargo_toml_should_be_denied("Cargo.toml", false)]
    #[case::target_binary_should_be_denied("target/debug/app", false)]
    fn resolver_with_src_policy_should_allow_only_matching_relative_paths(
        #[case] input: &str,
        #[case] expected_ok: bool,
    ) {
        let dir = setup_test_dir();
        let resolver = resolver_with_policy(&dir, "src/**");

        // One policy, multiple paths: only matching relative paths should resolve.
        let result = resolver.resolve(input);
        assert!(
            result.is_ok() == expected_ok,
            "path '{input}' should {}match 'src/**'",
            if expected_ok { "" } else { "not " }
        );
    }

    #[rstest]
    #[case::new_rs_file_should_be_allowed("new_file.rs", true)]
    #[case::new_txt_file_should_be_denied("new_file.txt", false)]
    fn resolver_should_check_policy_for_new_paths(#[case] input: &str, #[case] expected_ok: bool) {
        let dir = setup_test_dir();
        let resolver = resolver_with_policy(&dir, "*.rs");

        // New paths are checked against policy before the file exists.
        let result = resolver.resolve(input);
        assert!(
            result.is_ok() == expected_ok,
            "path '{input}' should {}match '*.rs'",
            if expected_ok { "" } else { "not " }
        );
    }

    #[test]
    fn returns_resolved_path_without_dotdots() {
        let dir = setup_test_dir();
        let resolver = AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap();

        let resolved = resolver.resolve("src/../Cargo.toml").unwrap();
        assert!(
            !resolved.to_string_lossy().contains(".."),
            "resolved path should not contain '..'"
        );
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
    ) {
        let dir = setup_src_globstar_dir();
        let resolver = resolver_with_policy(&dir, "src/**/*.rs");

        // Verify globstar behavior across shallow, deep, and denied paths.
        let result = resolver.resolve(input);
        assert!(
            result.is_ok() == expected_ok,
            "path '{input}' should {}match 'src/**/*.rs'",
            if expected_ok { "" } else { "not " }
        );
    }

    // --- external directory permission ---

    #[test]
    fn resolves_external_path_when_permission_allows() {
        let dir = setup_test_dir();
        let external_dir = TempDir::new().unwrap();
        let external_file = external_dir.path().join("external.txt");
        fs::write(&external_file, "content").unwrap();

        // Grant access to anything under external_dir.
        let pattern = format!("{}/*", external_dir.path().to_str().unwrap());
        let resolver = resolver_with_external_rule(&dir, &pattern, PermissionAction::Allow);

        let result = resolver.resolve(external_file.to_str().unwrap());
        let resolved = result.expect("external path allowed by permission should resolve");
        assert!(resolved.is_absolute(), "resolved path must be absolute");
        let expected = external_file.canonicalize().expect("file should exist");
        assert_eq!(resolved, expected, "resolved path must be canonical");
    }

    /// External paths are rejected whether explicitly denied or no ruleset is configured.
    #[rstest]
    #[case::deny_rule(true)]
    #[case::no_ruleset(false)]
    fn rejects_external_path_without_allow(#[case] with_deny_ruleset: bool) {
        let dir = setup_test_dir();
        let resolver = if with_deny_ruleset {
            resolver_with_external_rule(&dir, "*", PermissionAction::Deny)
        } else {
            AllowedGlobResolver::new(vec![dir.path().to_path_buf()]).unwrap()
        };
        let result = resolver.resolve("/some/external/path.txt");
        let err = result.expect_err("external path should be rejected");
        assert!(err.to_string().contains("not within allowed"));
    }

    #[test]
    fn rejects_relative_path_even_with_external_permission() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("external_directory", "*", PermissionAction::Allow).unwrap());

        // No base directories — external permission allows everything, but only
        // absolute paths. Relative paths must still be rejected.
        let resolver = AllowedGlobResolver::from_canonical(Vec::<PathBuf>::new())
            .with_external_permission(Arc::new(ruleset));

        let result = resolver.resolve("relative/path.txt");
        assert!(
            result.is_err(),
            "relative paths must not be resolved externally"
        );
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not within allowed"));
    }

    /// Permission checks the canonicalized path, not the raw input.
    /// Input like `{tmp}/allowed/../allowed/secret.txt` canonicalizes to
    /// `{tmp}/allowed/secret.txt` which matches the exact pattern.
    #[test]
    fn canonicalizes_path_before_permission_check() {
        let dir = setup_test_dir();
        let tmp = TempDir::new().unwrap();
        let subdir = tmp.path().join("allowed");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("secret.txt"), "content").unwrap();

        let pattern = format!("{}/allowed/secret.txt", tmp.path().to_str().unwrap());
        let resolver = resolver_with_external_rule(&dir, &pattern, PermissionAction::Allow);

        let input = format!(
            "{}/allowed/../allowed/secret.txt",
            tmp.path().to_str().unwrap()
        );
        let result = resolver.resolve(&input);
        assert!(
            result.is_ok(),
            "canonicalized path must match exact pattern"
        );
        let resolved = result.unwrap();
        assert!(
            resolved.ends_with("allowed/secret.txt"),
            "resolved path should be canonical: {:?}",
            resolved
        );
    }
}
