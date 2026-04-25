//! Glob pattern policy for path resolution.
//!
//! Defines ordered allow/deny glob patterns. Patterns are evaluated with
//! **last-match-wins** precedence using reverse iteration for efficiency.
//! If no patterns match, access is denied.
//!
//! When built via [`GlobPolicy::builder_with_base`], patterns may be relative
//! (joined with the base path), absolute (used as-is), or contain shell
//! variables (`~`, `$HOME`). When built via [`GlobPolicy::builder`], patterns
//! are used verbatim. Pattern syntax supports:
//! - `*` to match any number of characters within a path component
//! - `?` to match a single character
//! - `**` to match any number of path components (including `/`)
//!
//! ```rust
//! use reloaded_code_core::path::{GlobPolicy, GlobPolicyBuilder, RuleAction};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let policy = GlobPolicy::builder_with_base("/workspace")?
//!     .add("src/**/*.rs", RuleAction::Allow)?
//!     .add("target/**", RuleAction::Deny)?
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use super::normalize::{expand_shell, normalize_path};
use crate::error::{ToolError, ToolResult};
use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};

/// Action to take when a glob pattern matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// Allow access to the matched path.
    Allow,
    /// Deny access to the matched path.
    Deny,
}

/// Glob pattern policy for path resolution.
///
/// Patterns are evaluated with **last-match-wins** precedence using reverse
/// iteration for early exit. If no patterns match, access is denied.
///
/// # Example
///
/// ```
/// use reloaded_code_core::path::{GlobPolicy, GlobPolicyBuilder, RuleAction};
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let policy = GlobPolicy::builder()
///     .add("*.rs", RuleAction::Allow)?
///     .add("target/**", RuleAction::Deny)?
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct GlobPolicy {
    /// Pre-compiled glob matchers with their associated actions
    rules: Vec<(GlobMatcher, RuleAction)>,
    /// Compiled set for fast path rejection
    glob_set: GlobSet,
}

impl std::fmt::Debug for GlobPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobPolicy")
            .field("rules_count", &self.rules.len())
            .field("glob_set", &self.glob_set)
            .finish()
    }
}

impl GlobPolicy {
    /// Creates a new policy builder with no base path.
    ///
    /// Patterns added via [`GlobPolicyBuilder::add()`] are used verbatim:
    /// no shell expansion or base-path resolution is performed. For workspace-rooted
    /// resolution, use [`GlobPolicy::builder_with_base()`] instead.
    pub fn builder() -> GlobPolicyBuilder {
        GlobPolicyBuilder::new()
    }

    /// Creates a builder with a workspace root for resolving relative patterns.
    ///
    /// The base path is shell-expanded (`~`, `$HOME`, `$VAR`). Relative patterns
    /// added via [`GlobPolicyBuilder::add()`] are joined with this base path
    /// and normalized to forward slashes before compilation. Absolute patterns
    /// are used as-is.
    ///
    /// # Arguments
    ///
    /// - `base`: Workspace root path. Shell-expanded before use.
    ///
    /// # Returns
    ///
    /// `Ok(GlobPolicyBuilder)` with `base_path` set to the expanded absolute path.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if shell expansion fails (e.g., unset
    /// environment variable).
    pub fn builder_with_base(base: impl AsRef<Path>) -> ToolResult<GlobPolicyBuilder> {
        let expanded = expand_shell(&base.as_ref().to_string_lossy())?;
        Ok(GlobPolicyBuilder {
            base_path: Some(expanded),
            rules: Vec::new(),
        })
    }

    /// Checks if a normalized path string is allowed by this policy.
    ///
    /// The path must already be normalized to forward slashes. Patterns are
    /// evaluated with **last-match-wins** precedence using reverse iteration
    /// for early exit. If no patterns match, the path is denied.
    ///
    /// # Arguments
    ///
    /// * `normalized_path` - The already-normalized path string to check
    ///   (typically relative to base directory with forward slashes)
    ///
    /// # Returns
    ///
    /// `true` if the path is allowed by the last matching rule, `false` otherwise.
    #[inline]
    pub fn is_allowed(&self, normalized_path: &str) -> bool {
        if self.rules.is_empty() {
            return false;
        }

        // Single-rule fast path: skip GlobSet + loop when there's only one rule.
        if let [(matcher, action)] = self.rules.as_slice() {
            return matcher.is_match(normalized_path) && matches!(action, RuleAction::Allow);
        }

        // Speedup: Match against all globs at once.
        if !self.glob_set.is_match(normalized_path) {
            return false;
        }

        for (matcher, action) in self.rules.iter().rev() {
            if matcher.is_match(normalized_path) {
                return matches!(action, RuleAction::Allow);
            }
        }

        false
    }
}

/// Builder for constructing [`GlobPolicy`] instances.
#[derive(Debug)]
pub struct GlobPolicyBuilder {
    /// Optional workspace root. When set, relative patterns are joined with
    /// this path before compilation.
    base_path: Option<PathBuf>,
    rules: Vec<(Glob, RuleAction)>,
}

#[allow(clippy::derivable_impls)] // Explicit impl for clarity; base_path=None is required by spec
impl Default for GlobPolicyBuilder {
    fn default() -> Self {
        Self {
            base_path: None,
            rules: Vec::new(),
        }
    }
}

impl GlobPolicyBuilder {
    /// Creates a new empty policy builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves a glob pattern against the base path.
    ///
    /// 1. Shell-expand the pattern (`$HOME`, `~/`, `$VAR`, `${VAR:-default}`).
    /// 2. If `base_path` is set AND the expanded path is relative, join with base.
    /// 3. Normalize to forward slashes via [`normalize_path`].
    ///
    /// # Arguments
    ///
    /// - `pattern`: Glob pattern string, which may contain shell variables (`$HOME`,
    ///   `~/`, `$VAR`, `${VAR:-default}`) and be relative or absolute.
    ///
    /// # Returns
    ///
    /// A normalized absolute or relative path string with forward slashes, ready
    /// for [`Glob::new`].
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if shell expansion fails (e.g., unset
    /// environment variable in pattern).
    fn resolve_pattern(&self, pattern: &str) -> ToolResult<String> {
        let expanded = expand_shell(pattern)?;
        let path = Path::new(&expanded);

        let resolved = if let Some(ref base) = self.base_path {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            }
        } else {
            path.to_path_buf()
        };

        Ok(normalize_path(&resolved).into_owned())
    }

    /// Adds a pattern with the specified action.
    ///
    /// Patterns are evaluated in the order they are added with last-match-wins
    /// semantics. When the builder was created via [`GlobPolicy::builder_with_base()`],
    /// the pattern is shell-expanded (`~`, `$HOME`, `$VAR`) and, if relative, joined
    /// with the base path before compilation. Absolute patterns are used as-is.
    ///
    /// Pattern syntax:
    /// - `*` matches any number of characters
    /// - `?` matches exactly one character
    /// - `**` matches any number of path components (including zero)
    /// - `{a,b}` matches either `a` or `b`
    ///
    /// Patterns are matched against the entire relative path string, so `*` and
    /// `?` can match path separators (`/`). For example:
    /// - `*.rs` matches `src/lib.rs` because `*` can span across `/`
    /// - `*.rs` matches `main.rs` at the root level
    /// - `target/**` matches `target/debug/app` (and any depth under `target/`)
    ///
    /// # Arguments
    ///
    /// - `pattern`: The glob pattern string. Shell-expanded and resolved against
    ///   the base path when [`GlobPolicy::builder_with_base()`] was used.
    /// - `action`: The rule action (`Allow` or `Deny`).
    ///
    /// # Returns
    ///
    /// `Ok(Self)` on success, allowing method chaining.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if shell expansion fails.
    /// Returns [`ToolError::InvalidPattern`] if the pattern syntax is invalid.
    pub fn add(mut self, pattern: &str, action: RuleAction) -> ToolResult<Self> {
        let resolved = self.resolve_pattern(pattern)?;
        let glob = Glob::new(&resolved).map_err(|e| {
            ToolError::InvalidPattern(format!("invalid glob pattern '{}': {}", pattern, e))
        })?;
        self.rules.push((glob, action));
        Ok(self)
    }

    /// Adds an allow pattern.
    ///
    /// Convenience wrapper around [`Self::add()`] with [`RuleAction::Allow`].
    /// The pattern is shell-expanded and resolved against the base path when
    /// [`GlobPolicy::builder_with_base()`] was used. See [`Self::add()`] for
    /// full pattern-syntax details.
    ///
    /// # Arguments
    ///
    /// - `pattern`: The glob pattern string. Shell-expanded and resolved against
    ///   the base path when [`GlobPolicy::builder_with_base()`] was used.
    ///
    /// # Returns
    ///
    /// `Ok(Self)` on success, allowing method chaining.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if shell expansion fails.
    /// Returns [`ToolError::InvalidPattern`] if the pattern syntax is invalid.
    pub fn allow(self, pattern: &str) -> ToolResult<Self> {
        self.add(pattern, RuleAction::Allow)
    }

    /// Adds a deny pattern.
    ///
    /// Convenience wrapper around [`Self::add()`] with [`RuleAction::Deny`].
    /// The pattern is shell-expanded and resolved against the base path when
    /// [`GlobPolicy::builder_with_base()`] was used. See [`Self::add()`] for
    /// full pattern-syntax details.
    ///
    /// # Arguments
    ///
    /// - `pattern`: The glob pattern string. Shell-expanded and resolved against
    ///   the base path when [`GlobPolicy::builder_with_base()`] was used.
    ///
    /// # Returns
    ///
    /// `Ok(Self)` on success, allowing method chaining.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPath`] if shell expansion fails.
    /// Returns [`ToolError::InvalidPattern`] if the pattern syntax is invalid.
    pub fn deny(self, pattern: &str) -> ToolResult<Self> {
        self.add(pattern, RuleAction::Deny)
    }

    /// Builds the policy, compiling all patterns into a [`GlobSet`].
    ///
    /// # Returns
    ///
    /// `Ok(GlobPolicy)` with all patterns compiled for matching.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::InvalidPattern`] if any pattern fails to compile.
    pub fn build(self) -> ToolResult<GlobPolicy> {
        let mut builder = GlobSetBuilder::new();
        let mut rules = Vec::with_capacity(self.rules.len());

        for (glob, action) in self.rules {
            builder.add(glob.clone());
            rules.push((glob.compile_matcher(), action));
        }

        let glob_set = builder
            .build()
            .map_err(|e| ToolError::InvalidPattern(format!("failed to build glob set: {}", e)))?;

        Ok(GlobPolicy { rules, glob_set })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::error::Error;
    use std::fs;
    use temp_env;
    use tempfile::TempDir;

    #[rstest]
    #[case::src_lib_rs_allowed("src/lib.rs", true)]
    #[case::main_rs_allowed("main.rs", true)]
    #[case::target_debug_app_denied("target/debug/app", false)]
    #[case::cargo_toml_denied("Cargo.toml", false)]
    fn builder_should_apply_allow_and_deny_rules(
        #[case] path: &str,
        #[case] expected_allowed: bool,
    ) -> Result<(), Box<dyn Error>> {
        let policy = GlobPolicy::builder()
            .allow("*.rs")?
            .deny("target/**")?
            .build()?;

        assert_eq!(policy.is_allowed(path), expected_allowed);
        Ok(())
    }

    #[test]
    fn glob_policy_last_match_wins() -> Result<(), Box<dyn Error>> {
        let policy = GlobPolicy::builder()
            .deny("target/**")?
            .allow("target/debug/app")?
            .build()?;

        assert!(policy.is_allowed("target/debug/app"));
        assert!(!policy.is_allowed("target/release/app"));
        assert!(!policy.is_allowed("target/anything.txt"));

        let policy2 = GlobPolicy::builder()
            .allow("target/debug/app")?
            .deny("target/**")?
            .build()?;

        assert!(!policy2.is_allowed("target/debug/app"));
        Ok(())
    }

    #[test]
    fn invalid_glob_pattern_fails() {
        let result = GlobPolicy::builder().allow("[invalid");
        assert!(result.is_err());
    }

    #[rstest]
    #[case::anything_txt_denied("anything.txt")]
    #[case::src_lib_rs_denied("src/lib.rs")]
    fn empty_policy_should_deny_any_path(#[case] path: &str) -> Result<(), Box<dyn Error>> {
        let policy = GlobPolicy::builder().build()?;
        assert!(!policy.is_allowed(path));
        Ok(())
    }

    // Keep policy construction out of the case table so each case reads as
    // just input path + expected decision.
    fn src_rs_policy() -> Result<GlobPolicy, Box<dyn Error>> {
        Ok(GlobPolicy::builder().allow("src/**/*.rs")?.build()?)
    }

    #[rstest]
    #[case::root_level_lib_rs_allowed("src/lib.rs", true)]
    #[case::root_level_main_rs_allowed("src/main.rs", true)]
    #[case::nested_module_rs_allowed("src/deep/nested/module.rs", true)]
    #[case::nested_mod_rs_allowed("src/deep/nested/mod.rs", true)]
    #[case::deeply_nested_rs_allowed("src/a/b/c/d/e/file.rs", true)]
    #[case::wrong_extension_denied("src/lib.txt", false)]
    #[case::wrong_directory_denied("tests/test.rs", false)]
    #[case::parent_directory_denied("../src/lib.rs", false)]
    #[case::target_directory_denied("target/debug/lib.rs", false)]
    #[case::empty_path_denied("", false)]
    #[case::dot_path_denied(".", false)]
    #[case::dotdot_path_denied("..", false)]
    fn src_globstar_rs_policy_should_match_only_normalized_src_rs_paths(
        #[case] path: &str,
        #[case] expected_allowed: bool,
    ) -> Result<(), Box<dyn Error>> {
        let policy = src_rs_policy()?;

        let result = policy.is_allowed(path);
        assert_eq!(
            result,
            expected_allowed,
            "path '{}' should {}match 'src/**/*.rs'",
            path,
            if expected_allowed { "" } else { "not " }
        );
        Ok(())
    }

    // --- builder (no base path) ---

    #[test]
    fn default_builder_has_no_base_path() -> Result<(), Box<dyn Error>> {
        // Default builder uses patterns verbatim (no base-path resolution).
        let policy = GlobPolicyBuilder::default().allow("src/**/*.rs")?.build()?;
        // If base_path were Some(...), this relative path would not match.
        assert!(policy.is_allowed("src/lib.rs"));
        Ok(())
    }

    // Even without a base path, patterns still undergo shell expansion (~,
    // $HOME) so that "~/src/**" resolves to the absolute home directory.
    #[test]
    fn shell_expansion_in_pattern_without_base() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let temp_home = temp.path().canonicalize()?;

        temp_env::with_var("HOME", Some(&temp_home), || {
            let policy = GlobPolicy::builder().allow("$HOME/src/**")?.build()?;
            let abs_path = temp_home.join("src").join("lib.rs");
            let abs_path = normalize_path(&abs_path);
            assert!(policy.is_allowed(&abs_path));
            Ok(())
        })
    }

    // --- builder_with_base ---

    // A relative pattern like "src/**" is joined with the base path, so the
    // resulting compiled glob matches the absolute path under that base.
    #[test]
    fn relative_pattern_with_base_matches_absolute_path() -> Result<(), Box<dyn Error>> {
        let dir = TempDir::new()?;
        let base = dir.path().canonicalize()?;
        // Create a real file so the canonicalized base is a valid directory tree.
        fs::create_dir_all(dir.path().join("src"))?;
        fs::write(dir.path().join("src/lib.rs"), "content")?;

        // "src/**" is relative, so builder_with_base joins it with `base`
        // to produce an absolute glob like "/tmp/.tmpXXX/src/**".
        let policy = GlobPolicy::builder_with_base(&base)?
            .allow("src/**")?
            .build()?;

        // Reconstruct the absolute path the policy should match against.
        let abs_path = format!(
            "{}/src/lib.rs",
            base.to_str().unwrap().trim_end_matches('/')
        );
        assert!(
            policy.is_allowed(&abs_path),
            "relative pattern 'src/**' should match absolute path '{}'",
            abs_path
        );
        Ok(())
    }

    // Absolute patterns bypass base-path joining entirely - the leading `/`
    // means `resolve_pattern` uses the pattern verbatim regardless of base.
    #[test]
    fn absolute_pattern_with_base_used_as_is() -> Result<(), Box<dyn Error>> {
        let dir = TempDir::new()?;
        let base = dir.path().canonicalize()?;

        let (abs_pattern, file_path, other_path) = if cfg!(windows) {
            (
                "C:/some/absolute/path/**/*.rs",
                "C:/some/absolute/path/file.rs",
                "D:/other/path/file.rs",
            )
        } else {
            (
                "/some/absolute/path/**/*.rs",
                "/some/absolute/path/file.rs",
                "/other/path/file.rs",
            )
        };
        let policy = GlobPolicy::builder_with_base(&base)?
            .allow(abs_pattern)?
            .build()?;

        assert!(policy.is_allowed(file_path));
        assert!(!policy.is_allowed(other_path));
        Ok(())
    }

    // Shell expansion applies to the base path itself, not just patterns.
    // Here `$HOME/project` is expanded before becoming the base, then the
    // relative "src/**" pattern is joined against that resolved base.
    #[test]
    fn shell_expansion_in_pattern_with_base() -> Result<(), Box<dyn Error>> {
        let temp = TempDir::new()?;
        let temp_home = temp.path().canonicalize()?;
        let project_dir = temp.path().join("project");
        fs::create_dir_all(project_dir.join("src"))?;

        temp_env::with_var("HOME", Some(&temp_home), || {
            let policy = GlobPolicy::builder_with_base("$HOME/project")?
                .allow("src/**")?
                .build()?;

            // The final glob matches $HOME/project/src/**.
            let abs_path = format!(
                "{}/project/src/lib.rs",
                temp_home.to_str().unwrap().trim_end_matches('/')
            );
            assert!(policy.is_allowed(&abs_path));
            Ok(())
        })
    }

    #[test]
    fn builder_with_base_fails_on_invalid_shell_expansion() {
        temp_env::with_var("NONEXISTENT_VAR_99999", None::<&str>, || {
            let result = GlobPolicy::builder_with_base("$NONEXISTENT_VAR_99999/path");
            assert!(result.is_err());
        });
    }

    #[test]
    fn add_fails_on_invalid_shell_expansion_in_pattern() {
        temp_env::with_var("NONEXISTENT_VAR_99999", None::<&str>, || {
            let result = GlobPolicy::builder().allow("$NONEXISTENT_VAR_99999/**");
            assert!(result.is_err());
        });
    }
}
