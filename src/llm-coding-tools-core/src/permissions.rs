//! Permission evaluation for tool and delegation access control.
//!
//! This module provides a small, framework-agnostic policy model built from
//! ordered [`Rule`] entries inside a [`Ruleset`].
//!
//! - A rule is `(permission_key, subject_pattern, action)`.
//! - Evaluation is **last-match-wins**.
//! - If nothing matches, the result is [`PermissionAction::Deny`].
//!
//! Matching behaviour:
//! - Permission key: exact text, or a pattern using `*` (any number of
//!   characters, including none) and `?` (exactly one character).
//! - Subject pattern: same pattern rules: `*` means any number of
//!   characters (including none), and `?` means exactly one character.
//!
//! # Mapping config to rules
//!
//! Wrappers commonly map user-facing permission config (for example agent
//! frontmatter) into this model:
//!
//! - `bash: allow` maps to `Rule::new("bash", "*", Allow)`.
//! - `*: allow` maps to `Rule::new("*", "*", Allow)` (matches any tool).
//! - Pattern maps like `task: { "*": deny, "orchestrator-*": allow }`
//!   become one rule per pattern, in declaration order.
//!
//! Because matching is last-match-wins, rule order is part of policy.
//!
//! ```yaml
//! permission:
//!   "*": allow   # Allow all tools by default
//!   bash: deny   # But deny bash specifically (last match wins)
//!   task:
//!     "*": deny
//!     orchestrator-*: allow
//! ```
//!
//! ```rust
//! use llm_coding_tools_core::permissions::{ExpandError, PermissionAction, Rule, Ruleset};
//!
//! # fn main() -> Result<(), ExpandError> {
//! let mut rules = Ruleset::new();
//! rules.push(Rule::new("*", "*", PermissionAction::Allow)?);    // Allow all
//! rules.push(Rule::new("bash", "*", PermissionAction::Deny)?);  // Except bash
//! rules.push(Rule::new("task", "*", PermissionAction::Deny)?);  // Except task
//! rules.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow)?); // But allow `orchestrator-*` task
//!
//! assert_eq!(rules.evaluate("bash", "any-agent"), PermissionAction::Deny);
//! assert_eq!(rules.evaluate("read", "any-agent"), PermissionAction::Allow);
//! assert_eq!(
//!     rules.evaluate("task", "orchestrator-review"),
//!     PermissionAction::Allow
//! );
//! assert_eq!(rules.evaluate("task", "other-agent"), PermissionAction::Deny);
//! # Ok(())
//! # }
//! ```

use crate::internal::hash64::hash_u64;
use crate::internal::hash64::Hash64;
use crate::path::allowed_glob::normalize::expand_pattern;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Error returned when shell expansion of a rule pattern fails
/// (e.g., `$HOME` is unset).
pub type ExpandError = shellexpand::LookupError<std::env::VarError>;

/// Permission level for tool access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum PermissionAction {
    /// Tool is denied.
    #[default]
    Deny = 0,
    /// Tool is allowed.
    Allow = 1,
}

/// A single permission rule with pattern-based matching.
///
/// Fields are private to enforce invariants.
/// Use [`Rule::new`] to create rules.
///
/// # Memory Layout
///
/// Size is 56 bytes on 64-bit platforms:
/// - `permission`: 16 bytes (`Box<str>` ptr + len)
/// - `pattern`: 16 bytes (`Box<str>` ptr + len)
/// - `permission_hash`: 8 bytes (Hash64)
/// - `pattern_hash`: 8 bytes (Hash64)
/// - `permission_is_wildcard`: 1 byte
/// - `pattern_is_wildcard`: 1 byte
/// - `action`: 1 byte
/// - padding: 5 bytes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The permission key pattern (e.g., "bash", "*", "task-*")
    permission: Box<str>,
    /// The subject pattern (e.g., "*", "orchestrator-*")
    pattern: Box<str>,
    /// Pre-computed hash of `permission` for fast exact-match comparison
    permission_hash: Hash64,
    /// Pre-computed hash of `pattern` for fast exact-match comparison
    pattern_hash: Hash64,
    /// Whether `permission` uses `*` (any number of chars) or `?` (one char).
    permission_is_wildcard: bool,
    /// Whether `pattern` uses `*` (any number of chars) or `?` (one char).
    pattern_is_wildcard: bool,
    /// The action (Allow/Deny)
    action: PermissionAction,
}

impl Rule {
    /// Creates a new rule with the provided permission and pattern.
    ///
    /// Permission keys with `*` or `?` are treated as patterns.
    /// `*` matches any number of characters (including none), and `?`
    /// matches exactly one.
    ///
    /// # Examples
    ///
    /// ```
    /// use llm_coding_tools_core::permissions::{Rule, PermissionAction};
    ///
    /// // Exact match on permission key
    /// let exact = Rule::new("bash", "*", PermissionAction::Allow).unwrap();
    ///
    /// // Wildcard permission key matches any tool
    /// let wildcard = Rule::new("*", "*", PermissionAction::Allow).unwrap();
    /// ```
    pub fn new(
        permission: impl Into<Box<str>>,
        pattern: impl Into<Box<str>>,
        action: PermissionAction,
    ) -> Result<Self, ExpandError> {
        let permission = permission.into();
        let pattern_box: Box<str> = pattern.into();
        let pattern: Box<str> = match expand_pattern(&pattern_box) {
            Ok(Cow::Borrowed(_)) => pattern_box,
            Ok(Cow::Owned(s)) => s.into_boxed_str(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            permission_hash: hash_u64(&permission),
            pattern_hash: hash_u64(&pattern),
            permission_is_wildcard: permission.contains('*') || permission.contains('?'),
            pattern_is_wildcard: pattern.contains('*') || pattern.contains('?'),
            permission,
            pattern,
            action,
        })
    }

    /// Returns the permission key pattern.
    #[inline]
    pub fn permission(&self) -> &str {
        &self.permission
    }

    /// Returns the stored pattern.
    #[inline]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Returns the action for this rule.
    #[inline]
    pub fn action(&self) -> PermissionAction {
        self.action
    }

    /// Returns the stored 64-bit permission hash.
    #[inline]
    pub fn permission_hash(&self) -> u64 {
        self.permission_hash.as_u64()
    }

    /// Returns the stored 64-bit pattern hash.
    #[inline]
    pub fn pattern_hash(&self) -> u64 {
        self.pattern_hash.as_u64()
    }

    /// Returns true if the permission key contains wildcards.
    #[inline]
    pub fn permission_is_wildcard(&self) -> bool {
        self.permission_is_wildcard
    }

    /// Returns true if the pattern contains wildcards.
    #[inline]
    pub fn pattern_is_wildcard(&self) -> bool {
        self.pattern_is_wildcard
    }
}

/// Ordered ruleset for permission evaluation. Last matching rule wins.
///
/// # Default Behavior
///
/// When no rule matches, the default action is [`PermissionAction::Deny`].
/// To allow a permission, you must explicitly add an allow rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ruleset {
    rules: Vec<Rule>,
}

impl Ruleset {
    /// Creates an empty ruleset.
    #[inline]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Creates a ruleset with preallocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            rules: Vec::with_capacity(capacity),
        }
    }

    /// Appends a rule to the ruleset.
    #[inline]
    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Returns the number of rules.
    #[inline]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Returns true if the ruleset is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Returns an iterator over the rules.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Rule> {
        self.rules.iter()
    }

    /// Evaluates the ruleset for a given permission and subject.
    ///
    /// Returns the action from the last matching rule, or [`PermissionAction::Deny`]
    /// if no rule matches (default deny).
    ///
    /// Permission keys can be exact text, or patterns.
    /// In patterns, `*` means any number of characters (including none),
    /// and `?` means exactly one.
    ///
    /// # Arguments
    ///
    /// * `permission` - The permission key (tool name) to check;
    ///   `*` means any number of chars, `?` means one char.
    /// * `subject` - The subject to match against rule patterns (e.g., agent name, path)
    pub fn evaluate(&self, permission: &str, subject: &str) -> PermissionAction {
        match self.rules.as_slice() {
            [] => return PermissionAction::Deny,
            [rule] => return evaluate_single_rule(rule, permission, subject),
            _ => {}
        }

        let permission_hash = hash_u64(permission);
        let mut subject_hash = None;

        // Last-match-wins means the hottest successful path is the tail of the
        // ruleset, so scan in reverse and exit on the first match.
        for rule in self.rules.iter().rev() {
            if !rule_matches(
                permission,
                permission_hash,
                &rule.permission,
                rule.permission_hash,
                rule.permission_is_wildcard,
            ) {
                continue;
            }

            let pattern_matches = if rule.pattern_is_wildcard {
                wildcard_match(subject, &rule.pattern)
            } else {
                let subject_hash = *subject_hash.get_or_insert_with(|| hash_u64(subject));
                rule.pattern_hash == subject_hash && &*rule.pattern == subject
            };

            if pattern_matches {
                return rule.action;
            }
        }

        PermissionAction::Deny
    }

    /// Checks if a permission is allowed for the given subject.
    ///
    /// Convenience method that returns `true` if [`evaluate`](Self::evaluate)
    /// returns [`PermissionAction::Allow`].
    #[inline]
    pub fn is_allowed(&self, permission: &str, subject: &str) -> bool {
        self.evaluate(permission, subject) == PermissionAction::Allow
    }

    /// Merges another ruleset into this one.
    ///
    /// Rules from `other` are appended in order, giving them higher priority
    /// in last-match-wins evaluation.
    pub fn merge(&mut self, other: &Ruleset) {
        self.rules.reserve(other.rules.len());
        self.rules.extend(other.rules.iter().cloned());
    }

    /// Creates a new ruleset by merging multiple rulesets.
    ///
    /// Rules are concatenated in order; later rulesets have higher priority.
    pub fn merged<'a>(rulesets: impl IntoIterator<Item = &'a Ruleset>) -> Self {
        let rulesets: Vec<_> = rulesets.into_iter().collect();
        let capacity = rulesets.iter().map(|r| r.len()).sum();
        let mut result = Self::with_capacity(capacity);
        for ruleset in &rulesets {
            result.merge(ruleset);
        }
        result
    }
}

/// Matches a string against a wildcard pattern.
///
/// `*` matches any number of characters (including none), and `?`
/// matches exactly one character.
///
/// # Examples
///
/// ```ignore
/// assert!(wildcard_match("bash", "*"));
/// assert!(wildcard_match("orchestrator-builder", "orchestrator-*"));
/// assert!(wildcard_match("test", "te?t"));
/// assert!(!wildcard_match("bash", "task"));
/// ```
pub fn wildcard_match(input: &str, pattern: &str) -> bool {
    // Fast path: exact match or universal wildcard
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return input == pattern;
    }

    // Convert pattern to regex-like matching using a simple state machine
    // This avoids regex overhead for simple patterns
    wildcard_match_impl(input.as_bytes(), pattern.as_bytes())
}

/// Recursive wildcard matching implementation.
///
/// Uses byte slices for efficiency. Handles `*` and `?` wildcards.
fn wildcard_match_impl(input: &[u8], pattern: &[u8]) -> bool {
    let mut i = 0;
    let mut p = 0;
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0;

    while i < input.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == input[i]) {
            // Character match or single-char wildcard
            i += 1;
            p += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            // Star: save position and try zero-length match
            star_idx = Some(p);
            match_idx = i;
            p += 1;
        } else if let Some(star) = star_idx {
            // Backtrack: star matches ≥0 chars. Let star consume one more
            // character and retry matching the rest of the pattern from there.
            p = star + 1;
            match_idx += 1;
            i = match_idx;
        } else {
            // No match
            return false;
        }
    }

    // Consume trailing stars
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }

    p == pattern.len()
}

/// Matches an input against a rule value using hash+string or wildcard matching.
///
/// When `is_wildcard` is false, uses fast hash comparison followed by string
/// equality verification (for collision safety). When `is_wildcard` is true,
/// falls back to `wildcard_match()` for pattern matching.
#[inline(always)]
fn rule_matches(
    input: &str,
    input_hash: Hash64,
    rule_value: &str,
    rule_hash: Hash64,
    is_wildcard: bool,
) -> bool {
    if is_wildcard {
        wildcard_match(input, rule_value)
    } else {
        rule_hash == input_hash && rule_value == input
    }
}

#[inline(always)]
fn evaluate_single_rule(rule: &Rule, permission: &str, subject: &str) -> PermissionAction {
    let permission_hash = hash_u64(permission);
    if !rule_matches(
        permission,
        permission_hash,
        &rule.permission,
        rule.permission_hash,
        rule.permission_is_wildcard,
    ) {
        return PermissionAction::Deny;
    }

    let pattern_matches = if rule.pattern_is_wildcard {
        wildcard_match(subject, &rule.pattern)
    } else {
        rule.pattern_hash == hash_u64(subject) && &*rule.pattern == subject
    };

    if pattern_matches {
        rule.action
    } else {
        PermissionAction::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use temp_env;
    use tempfile::TempDir;

    type TestResult = Result<(), ExpandError>;

    fn build_and_eval(
        rules: &[(&str, &str, PermissionAction)],
        permission: &str,
        subject: &str,
    ) -> PermissionAction {
        let mut ruleset = Ruleset::new();
        for (perm, pat, action) in rules {
            ruleset.push(Rule::new(*perm, *pat, *action).expect(&format!(
                "failed to create Rule for permission {:?}, pattern {:?}, action {:?}",
                perm, pat, action
            )));
        }
        ruleset.evaluate(permission, subject)
    }

    // --- wildcard_match ---

    /// Verifies [`wildcard_match`] semantics (`*`, `?`, exact, and case sensitivity).
    #[rstest]
    #[case::star_should_match_empty(
        "",   // input: empty string
        "*",  // pattern: universal wildcard
        true  // expected: '*' matches any length, including empty
    )]
    #[case::exact_literals_should_match(
        "bash", // input: exact literal
        "bash", // pattern: same literal
        true    // expected: exact literals match
    )]
    #[case::different_literals_should_not_match(
        "bash", // input: literal text
        "task", // pattern: different literal text
        false   // expected: different literals do not match
    )]
    #[case::prefix_star_should_match_suffix(
        "orchestrator-builder", // input: starts with required prefix
        "orchestrator-*",       // pattern: fixed prefix plus wildcard suffix
        true                    // expected: suffix wildcard consumes the remainder
    )]
    #[case::prefix_star_should_not_match_wrong_prefix(
        "other-builder",  // input: wrong prefix
        "orchestrator-*", // pattern: requires "orchestrator-" prefix
        false             // expected: prefix requirement fails
    )]
    #[case::multiple_stars_should_backtrack(
        "ba-foobar-sh-ext", // input: requires star backtracking to fit both anchors
        "ba*sh*",           // pattern: anchored at "ba" then later "sh"
        true                // expected: matcher finds a valid star split
    )]
    #[case::question_mark_should_match_one_char(
        "test", // input: exactly one char between "te" and "t"
        "te?t", // pattern: single-char wildcard in the middle
        true    // expected: '?' matches exactly one character
    )]
    #[case::question_mark_should_not_match_two_chars(
        "teest", // input: two chars between "te" and "t"
        "te?t",  // pattern: allows exactly one char in the middle
        false    // expected: '?' cannot consume two characters
    )]
    #[case::matching_should_be_case_sensitive(
        "TOOL-read", // input: uppercase prefix
        "tool-*",    // pattern: lowercase prefix
        false        // expected: matching is case-sensitive
    )]
    fn wildcard_match_cases(#[case] input: &str, #[case] pattern: &str, #[case] expected: bool) {
        assert_eq!(wildcard_match(input, pattern), expected);
    }

    // --- Rule construction / metadata ---

    /// A plain permission key without `*` or `?` must not set the wildcard flag.
    #[test]
    fn exact_key_should_not_set_wildcard_flag() -> TestResult {
        let rule = Rule::new("bash", "*", PermissionAction::Allow)?;
        assert_eq!(rule.permission(), "bash");
        assert_eq!(rule.permission_hash(), hash_u64("bash").as_u64());
        assert!(!rule.permission_is_wildcard());
        assert_eq!(rule.action(), PermissionAction::Allow);
        Ok(())
    }

    /// A lone `*` permission key must set the wildcard flag.
    #[test]
    fn star_key_should_set_wildcard_flag() -> TestResult {
        let rule = Rule::new("*", "*", PermissionAction::Allow)?;
        assert_eq!(rule.permission(), "*");
        assert_eq!(rule.permission_hash(), hash_u64("*").as_u64());
        assert!(rule.permission_is_wildcard());
        Ok(())
    }

    /// A permission key like `"bash*"` ends with a wildcard and must set the flag.
    #[test]
    fn partial_wildcard_key_should_set_wildcard_flag() -> TestResult {
        let rule = Rule::new("bash*", "*", PermissionAction::Allow)?;
        assert_eq!(rule.permission(), "bash*");
        assert_eq!(rule.permission_hash(), hash_u64("bash*").as_u64());
        assert!(rule.permission_is_wildcard());
        Ok(())
    }

    /// A subject pattern containing `*` must set the pattern wildcard flag, leaving permission untouched.
    #[test]
    fn wildcard_subject_should_set_wildcard_flag() -> TestResult {
        let rule = Rule::new("bash", "orchestrator-*", PermissionAction::Allow)?;
        assert_eq!(rule.pattern(), "orchestrator-*");
        assert_eq!(rule.pattern_hash(), hash_u64("orchestrator-*").as_u64());
        assert!(rule.pattern_is_wildcard());
        assert!(!rule.permission_is_wildcard());
        Ok(())
    }

    /// A plain subject string without wildcards must not set the pattern wildcard flag.
    #[test]
    fn exact_subject_should_not_set_wildcard_flag() -> TestResult {
        let rule = Rule::new("bash", "exact-subject", PermissionAction::Allow)?;
        assert_eq!(rule.pattern(), "exact-subject");
        assert_eq!(rule.pattern_hash(), hash_u64("exact-subject").as_u64());
        assert!(!rule.pattern_is_wildcard());
        assert!(!rule.permission_is_wildcard());
        Ok(())
    }

    #[test]
    fn rule_permission_hash_should_be_case_sensitive() -> TestResult {
        let upper = Rule::new("BASH", "*", PermissionAction::Allow)?;
        let lower = Rule::new("bash", "*", PermissionAction::Allow)?;
        assert_ne!(upper.permission_hash(), lower.permission_hash());
        Ok(())
    }

    #[test]
    fn rule_pattern_hash_should_be_case_sensitive() -> TestResult {
        let upper = Rule::new("bash", "SUBJECT", PermissionAction::Allow)?;
        let lower = Rule::new("bash", "subject", PermissionAction::Allow)?;
        assert_ne!(upper.pattern_hash(), lower.pattern_hash());
        Ok(())
    }

    #[test]
    fn rule_should_be_56_byte_clone() {
        assert_eq!(std::mem::size_of::<Rule>(), 56);
        fn assert_clone<T: Clone>() {}
        assert_clone::<Rule>();
    }

    #[test]
    fn evaluate_when_no_rules_should_deny() {
        assert_eq!(
            build_and_eval(&[], "bash", "anything"),
            PermissionAction::Deny,
        );
    }

    #[test]
    fn evaluate_exact_match_should_allow() {
        assert_eq!(
            build_and_eval(
                &[("bash", "*", PermissionAction::Allow)],
                "bash",
                "anything"
            ),
            PermissionAction::Allow,
        );
    }

    #[test]
    fn evaluate_should_be_case_sensitive() {
        assert_eq!(
            build_and_eval(
                &[("BASH", "*", PermissionAction::Allow)],
                "bash",
                "anything"
            ),
            PermissionAction::Deny,
        );
    }

    #[test]
    fn evaluate_last_matching_rule_should_win() {
        assert_eq!(
            build_and_eval(
                &[
                    ("task", "*", PermissionAction::Deny),
                    ("task", "orchestrator-*", PermissionAction::Allow),
                ],
                "task",
                "orchestrator-builder",
            ),
            PermissionAction::Allow,
        );
    }

    #[test]
    fn evaluate_non_matching_rule_should_not_override() {
        assert_eq!(
            build_and_eval(
                &[
                    ("task", "*", PermissionAction::Deny),
                    ("task", "orchestrator-*", PermissionAction::Allow),
                ],
                "task",
                "random-agent",
            ),
            PermissionAction::Deny,
        );
    }

    #[test]
    fn evaluate_star_permission_should_match_any_tool() {
        assert_eq!(
            build_and_eval(&[("*", "*", PermissionAction::Allow)], "bash", "anything"),
            PermissionAction::Allow,
        );
    }

    #[test]
    fn evaluate_prefix_wildcard_permission_should_match() {
        assert_eq!(
            build_and_eval(
                &[("bash*", "*", PermissionAction::Allow)],
                "bash-extended",
                "anything"
            ),
            PermissionAction::Allow,
        );
    }

    #[test]
    fn evaluate_question_mark_permission_should_match() {
        assert_eq!(
            build_and_eval(
                &[("te?t", "*", PermissionAction::Allow)],
                "test",
                "anything"
            ),
            PermissionAction::Allow,
        );
    }

    #[test]
    fn evaluate_wildcard_subject_should_match() {
        assert_eq!(
            build_and_eval(
                &[("task", "orchestrator-*", PermissionAction::Allow)],
                "task",
                "orchestrator-builder",
            ),
            PermissionAction::Allow,
        );
    }

    #[test]
    fn evaluate_both_fields_must_match() {
        assert_eq!(
            build_and_eval(
                &[("*", "orchestrator-*", PermissionAction::Allow)],
                "bash",
                "other-agent",
            ),
            PermissionAction::Deny,
        );
    }

    // --- Ruleset convenience ---

    #[test]
    fn is_allowed_should_reflect_evaluate() -> TestResult {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("bash", "*", PermissionAction::Allow)?);
        assert!(ruleset.is_allowed("bash", "any"));
        assert!(!ruleset.is_allowed("task", "any"));
        Ok(())
    }

    #[test]
    fn merge_should_append_and_override() -> TestResult {
        let mut base = Ruleset::new();
        base.push(Rule::new("bash", "*", PermissionAction::Deny)?);

        let mut override_rules = Ruleset::new();
        override_rules.push(Rule::new("bash", "*", PermissionAction::Allow)?);

        base.merge(&override_rules);
        assert_eq!(base.evaluate("bash", "any"), PermissionAction::Allow);
        Ok(())
    }

    #[test]
    fn merged_should_concatenate_in_order() -> TestResult {
        let mut r1 = Ruleset::new();
        r1.push(Rule::new("a", "*", PermissionAction::Deny)?);

        let mut r2 = Ruleset::new();
        r2.push(Rule::new("a", "*", PermissionAction::Allow)?);

        let combined = Ruleset::merged([&r1, &r2]);
        assert_eq!(combined.evaluate("a", "x"), PermissionAction::Allow);
        Ok(())
    }

    // --- Rule::new with expansion integration ---

    /// Verifies that a rule created with `~/` pattern matches the expanded home path.
    #[cfg(not(windows))]
    #[test]
    fn rule_with_tilde_pattern_should_match_expanded_path() -> TestResult {
        let temp_dir = TempDir::new().unwrap();
        let temp_home = temp_dir.path().canonicalize().unwrap();

        temp_env::with_var("HOME", Some(&temp_home), || -> TestResult {
            let mut ruleset = Ruleset::new();
            ruleset.push(Rule::new("read", "~/projects/*", PermissionAction::Allow)?);

            let subject = format!("{}/projects/src/lib.rs", temp_home.to_str().unwrap());
            assert_eq!(
                ruleset.evaluate("read", &subject),
                PermissionAction::Allow,
                "expanded ~/ pattern should match real path"
            );
            Ok(())
        })
    }

    /// Verifies that a rule created with `$HOME/` pattern matches the expanded path.
    #[test]
    fn rule_with_dollar_home_pattern_should_match_expanded_path() -> TestResult {
        let temp_dir = TempDir::new().unwrap();
        let temp_home = temp_dir.path().canonicalize().unwrap();

        temp_env::with_var("HOME", Some(&temp_home), || -> TestResult {
            let mut ruleset = Ruleset::new();
            ruleset.push(Rule::new(
                "read",
                "$HOME/.config/*",
                PermissionAction::Allow,
            )?);

            let subject = format!("{}/.config/app.conf", temp_home.to_str().unwrap());
            assert_eq!(
                ruleset.evaluate("read", &subject),
                PermissionAction::Allow,
                "expanded $HOME/ pattern should match real path"
            );
            Ok(())
        })
    }

    /// Verifies that rules without shell patterns evaluate correctly after
    /// the expansion change (regression guard).
    #[test]
    fn rule_without_shell_patterns_evaluates_correctly() -> TestResult {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("bash", "*", PermissionAction::Allow)?);
        ruleset.push(Rule::new(
            "task",
            "orchestrator-*",
            PermissionAction::Allow,
        )?);

        assert_eq!(
            ruleset.evaluate("bash", "anything"),
            PermissionAction::Allow
        );
        assert_eq!(
            ruleset.evaluate("task", "orchestrator-builder"),
            PermissionAction::Allow
        );
        assert_eq!(ruleset.evaluate("task", "other"), PermissionAction::Deny);
        Ok(())
    }
}
