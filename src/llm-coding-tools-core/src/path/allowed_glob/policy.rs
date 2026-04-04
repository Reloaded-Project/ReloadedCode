//! Glob pattern policy for path resolution.
//!
//! Defines ordered allow/deny glob patterns. Patterns are evaluated with
//! **last-match-wins** precedence using reverse iteration for efficiency.
//! If no patterns match, access is denied.
//!
//! Patterns are always relative to the base directory (root-relative) and
//! support:
//! - `*` to match any number of characters within a path component
//! - `?` to match a single character
//! - `**` to match any number of path components (including `/`)

use crate::error::{ToolError, ToolResult};
use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};

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
/// use llm_coding_tools_core::path::{GlobPolicy, GlobPolicyBuilder, RuleAction};
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
    /// Creates a new policy builder.
    pub fn builder() -> GlobPolicyBuilder {
        GlobPolicyBuilder::new()
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
    pub(crate) fn is_allowed(&self, normalized_path: &str) -> bool {
        if self.rules.is_empty() {
            return false;
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
#[derive(Debug, Default)]
pub struct GlobPolicyBuilder {
    rules: Vec<(Glob, RuleAction)>,
}

impl GlobPolicyBuilder {
    /// Creates a new empty policy builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a pattern with the specified action.
    ///
    /// Patterns are evaluated in the order they are added with last-match-wins
    /// semantics. Patterns are always relative to the base directory and do
    /// NOT support `~/` or `$HOME/` prefixes (use home expansion in base
    /// directory paths instead).
    ///
    /// Pattern syntax:
    /// - `*` matches any number of characters (except `/`)
    /// - `?` matches exactly one character (except `/`)
    /// - `**` matches any number of characters including `/`
    /// - `{a,b}` matches either `a` or `b`
    ///
    /// # Arguments
    ///
    /// * `pattern` - The glob pattern string (relative to base directory, no
    ///   home expansion)
    /// * `action` - The rule action (`Allow` or `Deny`)
    ///
    /// # Returns
    ///
    /// `Ok(Self)` on success, allowing method chaining.
    ///
    /// # Errors
    ///
    /// Returns `ToolError::InvalidPattern` if the pattern syntax is invalid.
    pub fn add(mut self, pattern: &str, action: RuleAction) -> ToolResult<Self> {
        let glob = Glob::new(pattern).map_err(|e| {
            ToolError::InvalidPattern(format!("invalid glob pattern '{}': {}", pattern, e))
        })?;
        self.rules.push((glob, action));
        Ok(self)
    }

    /// Adds an allow pattern.
    ///
    /// This is a convenience wrapper around `add()` with `RuleAction::Allow`.
    /// See `add()` for pattern syntax and behavior details.
    ///
    /// Patterns are relative to base directory and do NOT support `~/` or
    /// `$HOME/` prefixes.
    pub fn allow(self, pattern: &str) -> ToolResult<Self> {
        self.add(pattern, RuleAction::Allow)
    }

    /// Adds a deny pattern.
    ///
    /// This is a convenience wrapper around `add()` with `RuleAction::Deny`.
    /// See `add()` for pattern syntax and behavior details.
    ///
    /// Patterns are relative to base directory and do NOT support `~/` or
    /// `$HOME/` prefixes.
    pub fn deny(self, pattern: &str) -> ToolResult<Self> {
        self.add(pattern, RuleAction::Deny)
    }

    /// Builds the policy, compiling all patterns.
    ///
    /// # Errors
    ///
    /// Returns `ToolError::InvalidPattern` if any pattern fails to compile.
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

    #[rstest]
    #[case::src_lib_rs_allowed("src/lib.rs", true)]
    #[case::main_rs_allowed("main.rs", true)]
    #[case::target_debug_app_denied("target/debug/app", false)]
    #[case::cargo_toml_denied("Cargo.toml", false)]
    fn builder_should_apply_allow_and_deny_rules(
        #[case] path: &str,
        #[case] expected_allowed: bool,
    ) {
        let policy = GlobPolicy::builder()
            .allow("*.rs")
            .unwrap()
            .deny("target/**")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(policy.is_allowed(path), expected_allowed);
    }

    #[test]
    fn glob_policy_last_match_wins() {
        let policy = GlobPolicy::builder()
            .deny("target/**")
            .unwrap()
            .allow("target/debug/app")
            .unwrap()
            .build()
            .unwrap();

        assert!(policy.is_allowed("target/debug/app"));
        assert!(!policy.is_allowed("target/release/app"));
        assert!(!policy.is_allowed("target/anything.txt"));

        let policy2 = GlobPolicy::builder()
            .allow("target/debug/app")
            .unwrap()
            .deny("target/**")
            .unwrap()
            .build()
            .unwrap();

        assert!(!policy2.is_allowed("target/debug/app"));
    }

    #[test]
    fn invalid_glob_pattern_fails() {
        let result = GlobPolicy::builder().allow("[invalid");
        assert!(result.is_err());
    }

    #[rstest]
    #[case::anything_txt_denied("anything.txt")]
    #[case::src_lib_rs_denied("src/lib.rs")]
    fn empty_policy_should_deny_any_path(#[case] path: &str) {
        let policy = GlobPolicy::builder().build().unwrap();
        assert!(!policy.is_allowed(path));
    }

    // Keep policy construction out of the case table so each case reads as
    // just input path + expected decision.
    fn src_rs_policy() -> GlobPolicy {
        GlobPolicy::builder()
            .allow("src/**/*.rs")
            .unwrap()
            .build()
            .unwrap()
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
    ) {
        let policy = src_rs_policy();

        let result = policy.is_allowed(path);
        assert_eq!(
            result,
            expected_allowed,
            "path '{}' should {}match 'src/**/*.rs'",
            path,
            if expected_allowed { "" } else { "not " }
        );
    }
}
