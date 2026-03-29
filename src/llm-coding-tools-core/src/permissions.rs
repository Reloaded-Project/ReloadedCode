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
//! - Permission key: exact match (case-sensitive), no wildcard expansion.
//! - Subject pattern: wildcard matching with `*` (many chars) and `?` (one char).
//!
//! # Mapping config to rules
//!
//! Wrappers commonly map user-facing permission config (for example agent
//! frontmatter) into this model:
//!
//! - `bash: allow` maps to `Rule::new("bash", "*", Allow)`.
//! - Pattern maps like `task: { "*": deny, "orchestrator-*": allow }`
//!   become one rule per pattern, in declaration order.
//!
//! Because matching is last-match-wins, rule order is part of policy.
//!
//! ```yaml
//! permission:
//!   bash: allow
//!   task:
//!     "*": deny
//!     orchestrator-*: allow
//! ```
//!
//! ```rust
//! use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};
//!
//! let mut rules = Ruleset::new();
//! rules.push(Rule::new("bash", "*", PermissionAction::Allow));
//! rules.push(Rule::new("task", "*", PermissionAction::Deny));
//! rules.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));
//!
//! assert_eq!(rules.evaluate("bash", "any-agent"), PermissionAction::Allow);
//! assert_eq!(
//!     rules.evaluate("task", "orchestrator-review"),
//!     PermissionAction::Allow
//! );
//! assert_eq!(rules.evaluate("task", "other-agent"), PermissionAction::Deny);
//! assert_eq!(rules.evaluate("read", "any-agent"), PermissionAction::Deny);
//! ```

use serde::{Deserialize, Serialize};
use tinyvec_string::TinyString;

use crate::internal::hash63::Hash63;
use crate::internal::hash64::hash_u64;
use crate::internal::packed_permission::PackedPermission;

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
/// Fields are private to enforce packing invariants.
/// Use [`Rule::new`] to create rules.
///
/// # Memory Optimizations
///
/// See: <https://github.com/Sewer56/llm-coding-tools/pull/32>
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Packed permission hash and action.
    permission: PackedPermission,
    /// Pattern to match against (e.g., "*", "orchestrator-*").
    pattern: TinyString<[u8; 14]>,
}

impl Rule {
    /// Creates a new rule with the provided permission and pattern.
    #[inline]
    pub fn new(
        permission: impl AsRef<str>,
        pattern: impl AsRef<str>,
        action: PermissionAction,
    ) -> Self {
        Self {
            permission: PackedPermission::new(hash_u64(permission.as_ref()), action),
            pattern: TinyString::<[u8; 14]>::from(pattern.as_ref()),
        }
    }

    /// Returns the stored 63-bit permission hash.
    #[inline]
    pub fn permission_hash(&self) -> u64 {
        self.permission.hash().as_u64()
    }

    /// Returns the stored pattern.
    #[inline]
    pub fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    /// Returns the action for this rule.
    #[inline]
    pub fn action(&self) -> PermissionAction {
        self.permission.action()
    }
}

/// Ordered ruleset for permission evaluation. Last matching rule wins.
///
/// # Default Behavior
///
/// When no rule matches, the default action is [`PermissionAction::Deny`].
/// To allow a permission, you must explicitly add an allow rule.
#[derive(Debug, Clone, Default)]
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
    /// Permission keys are matched with **exact equality**.
    /// Patterns are matched against subjects using wildcard matching.
    ///
    /// # Arguments
    ///
    /// * `permission` - The permission key (tool name) to check (exact match)
    /// * `subject` - The subject to match against rule patterns (e.g., agent name, path)
    pub fn evaluate(&self, permission: &str, subject: &str) -> PermissionAction {
        let permission_hash = Hash63::from_hash64(hash_u64(permission));

        // Last-match-wins: iterate forward, keep overwriting result
        let mut result = PermissionAction::Deny;

        for rule in &self.rules {
            // Permission key: exact match only (no wildcards)
            // Pattern: wildcard match against subject
            if rule.permission.hash() == permission_hash
                && wildcard_match(subject, rule.pattern.as_str())
            {
                result = rule.permission.action();
            }
        }

        result
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
        for ruleset in rulesets {
            result.merge(ruleset);
        }
        result
    }
}

/// Matches a string against a wildcard pattern.
///
/// Supports `*` (matches any sequence) and `?` (matches single char).
///
/// # Examples
///
/// ```ignore
/// assert!(wildcard_match("bash", "*"));
/// assert!(wildcard_match("orchestrator-builder", "orchestrator-*"));
/// assert!(wildcard_match("test", "te?t"));
/// assert!(!wildcard_match("bash", "task"));
/// ```
pub(crate) fn wildcard_match(input: &str, pattern: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// Verifies that wildcard patterns match inputs correctly for various
    /// pattern types: star (*), question mark (?), and combinations.
    #[rstest]
    // Star matches any string including empty
    #[case::star_matches_anything(
        "anything",  // input: any non-empty string
        "*",         // pattern: single star matches everything
        true         // matches: yes
    )]
    #[case::star_matches_empty(
        "",          // input: empty string
        "*",         // pattern: star matches empty too
        true         // matches: yes
    )]
    // Exact match requires identical strings
    #[case::exact_match(
        "bash",      // input: exact string
        "bash",      // pattern: identical string
        true         // matches: yes (exact match)
    )]
    #[case::exact_mismatch(
        "bash",      // input: one string
        "task",      // pattern: different string
        false        // matches: no (not identical)
    )]
    // Prefix patterns match from start
    #[case::prefix_star(
        "orchestrator-builder", // input: starts with prefix
        "orchestrator-*",       // pattern: prefix + star suffix
        true                    // matches: yes (prefix matches)
    )]
    #[case::prefix_star_match_empty_suffix(
        "orchestrator-",  // input: prefix only, empty suffix
        "orchestrator-*", // pattern: prefix + star
        true              // matches: yes (star matches empty)
    )]
    #[case::prefix_star_no_match(
        "other-builder",  // input: different prefix
        "orchestrator-*", // pattern: specific prefix required
        false             // matches: no (wrong prefix)
    )]
    // Suffix patterns match from end
    #[case::suffix_star(
        "pre-bash",  // input: ends with suffix
        "*-bash",    // pattern: star prefix + suffix
        true         // matches: yes (suffix matches)
    )]
    #[case::suffix_star_match_empty_prefix(
        "-bash",     // input: suffix only, empty prefix
        "*-bash",    // pattern: star + suffix
        true         // matches: yes (star matches empty)
    )]
    #[case::suffix_star_no_match(
        "bash-post", // input: different ending
        "*-bash",    // pattern: specific suffix required
        false        // matches: no (wrong suffix)
    )]
    // Middle star matches substring
    #[case::middle_star(
        "a-middle-z", // input: middle substring present
        "a-*-z",      // pattern: prefix + star + suffix
        true          // matches: yes (middle exists)
    )]
    #[case::middle_star_empty_middle(
        "a--z",     // input: empty middle (adjacent delimiters)
        "a-*-z",    // pattern: prefix + star + suffix
        true        // matches: yes (star matches empty)
    )]
    #[case::middle_star_no_match(
        "a-middle", // input: missing suffix
        "a-*-z",    // pattern: requires suffix
        false       // matches: no (no suffix)
    )]
    // Question mark matches exactly one character
    #[case::question_mark(
        "test", // input: one char at position 3
        "te?t", // pattern: te + ? + t
        true    // matches: yes (s matches ?)
    )]
    #[case::question_mark_another(
        "teat", // input: different char at position 3
        "te?t", // pattern: te + ? + t
        true    // matches: yes (a matches ?)
    )]
    #[case::question_mark_too_many(
        "teest", // input: two chars at position 3
        "te?t",  // pattern: exactly one char required
        false    // matches: no (too many chars)
    )]
    #[case::question_mark_too_few(
        "tet",  // input: zero chars at position 3
        "te?t", // pattern: exactly one char required
        false   // matches: no (too few chars)
    )]
    // Multiple stars work together
    #[case::multiple_stars_path(
        "a/b/c", // input: path with slash
        "*/*",   // pattern: two stars match components
        true     // matches: yes
    )]
    #[case::multiple_stars_complex(
        "abc",   // input: surrounded chars
        "*a*c*", // pattern: stars on both sides
        true     // matches: yes
    )]
    // Empty input edge cases
    #[case::empty_input_with_star(
        "",    // input: empty string
        "*",   // pattern: star matches empty
        true   // matches: yes
    )]
    #[case::empty_input_with_question(
        "",    // input: empty string
        "?",   // pattern: requires one char
        false  // matches: no (empty has zero)
    )]
    #[case::empty_input_with_literal(
        "",    // input: empty string
        "a",   // pattern: requires literal "a"
        false  // matches: no (not equal)
    )]
    fn wildcard_pattern_matching(
        #[case] input: &str,
        #[case] pattern: &str,
        #[case] should_match: bool,
    ) {
        assert_eq!(wildcard_match(input, pattern), should_match);
    }

    /// Verifies that Rule preserves casing and computes correct hashes.
    #[rstest]
    #[case::pattern_casing(
        "BASH",      // permission: uppercase permission key
        "PATTERN",   // pattern: uppercase pattern string
        "PATTERN"    // expected: pattern unchanged (preserves casing)
    )]
    #[case::lowercase_pattern(
        "bash",      // permission: lowercase permission key
        "pattern",   // pattern: lowercase pattern string
        "pattern"    // expected: pattern unchanged (preserves casing)
    )]
    fn rule_preserves_casing(
        #[case] permission: &str,
        #[case] pattern: &str,
        #[case] expected: &str,
    ) {
        let rule = Rule::new(permission, pattern, PermissionAction::Allow);
        assert_eq!(rule.pattern(), expected);
    }

    #[test]
    fn rule_permission_hash_is_case_sensitive() {
        let upper = Rule::new("BASH", "*", PermissionAction::Allow);
        let lower = Rule::new("bash", "*", PermissionAction::Allow);
        assert_ne!(upper.permission_hash(), lower.permission_hash());
    }

    #[test]
    fn rule_getters_return_correct_values() {
        let rule = Rule::new("task", "orchestrator-*", PermissionAction::Allow);
        assert_eq!(rule.pattern(), "orchestrator-*");
        assert_eq!(rule.action(), PermissionAction::Allow);
    }

    #[test]
    fn rule_size_is_32_bytes() {
        assert_eq!(std::mem::size_of::<Rule>(), 32);
    }

    /// Verifies that Ruleset evaluates rules correctly, including default deny
    /// and last-match-wins behaviour.
    #[rstest]
    // Default is deny when no rules match
    #[case::default_deny(
        vec![],                // rules: empty ruleset
        ("bash", "anything"),  // evaluate: permission="bash", subject="anything"
        PermissionAction::Deny // expected: default deny (no matching rules)
    )]
    // Simple allow rule permits matching permission
    #[case::simple_allow(
        vec![("bash", "*", PermissionAction::Allow)], // rules: allow all for "bash" permission
        ("bash", "anything"),                         // evaluate: matches permission="bash"
        PermissionAction::Allow                       // expected: allow (rule matches)
    )]
    // Non-matching permission is still denied
    #[case::simple_allow_nonmatch(
        vec![("bash", "*", PermissionAction::Allow)], // rules: allow all for "bash" only
        ("task", "anything"),                         // evaluate: different permission="task"
        PermissionAction::Deny                        // expected: deny (no matching rules)
    )]
    // Last match wins when multiple rules apply
    #[case::last_match_wins_allow(
        vec![
            ("task", "*", PermissionAction::Deny),              // rules[0]: deny all task
            ("task", "orchestrator-*", PermissionAction::Allow) // rules[1]: allow orchestrator-*
        ],
        ("task", "orchestrator-builder"),                       // evaluate: matches both rules
        PermissionAction::Allow                                 // expected: allow (rules[1] wins)
    )]
    #[case::last_match_wins_deny(
        vec![
            ("task", "orchestrator-*", PermissionAction::Allow),  // rules[0]: allow orchestrator-*
            ("task", "*", PermissionAction::Deny)                 // rules[1]: deny all task
        ],
        ("task", "orchestrator-builder"),                         // evaluate: matches both rules
        PermissionAction::Deny                                    // expected: deny (rules[1] wins)
    )]
    // Only first rule applies when second doesn't match
    #[case::partial_match_first_rule(
        vec![
            ("task", "*", PermissionAction::Deny),              // rules[0]: deny all task (matches)
            ("task", "orchestrator-*", PermissionAction::Allow) // rules[1]: allow orchestrator-* (doesn't match)
        ],
        ("task", "random-agent"),                              // evaluate: only matches rules[0]
        PermissionAction::Deny                                 // expected: deny (only first rule matches)
    )]
    fn ruleset_evaluate(
        #[case] rules: Vec<(&str, &str, PermissionAction)>,
        #[case] (permission, target): (&str, &str),
        #[case] expected: PermissionAction,
    ) {
        let mut ruleset = Ruleset::new();
        for (perm, pat, action) in rules {
            ruleset.push(Rule::new(perm, pat, action));
        }
        assert_eq!(ruleset.evaluate(permission, target), expected);
    }

    /// Verifies that permission keys and patterns are case-sensitive.
    #[rstest]
    #[case::permission_uppercase(
        vec![("BASH", "*", PermissionAction::Allow)], // rules: allow all for "BASH" permission
        ("BASH", "test"),                             // evaluate: exact case match
        PermissionAction::Allow                       // expected: allow (exact case match)
    )]
    #[case::permission_lowercase_no_match(
        vec![("BASH", "*", PermissionAction::Allow)], // rules: allow all for "BASH" only
        ("bash", "test"),                             // evaluate: different case "bash"
        PermissionAction::Deny                        // expected: deny (case mismatch)
    )]
    #[case::pattern_uppercase(
        vec![("task", "AGENT-*", PermissionAction::Allow)], // rules: allow AGENT-* pattern
        ("task", "AGENT-foo"),                              // evaluate: exact case match
        PermissionAction::Allow                             // expected: allow (exact case match)
    )]
    #[case::pattern_lowercase_no_match(
        vec![("task", "AGENT-*", PermissionAction::Allow)], // rules: allow AGENT-* only
        ("task", "agent-foo"),                              // evaluate: different case "agent-foo"
        PermissionAction::Deny                              // expected: deny (case mismatch)
    )]
    fn ruleset_evaluate_case_sensitive(
        #[case] rules: Vec<(&str, &str, PermissionAction)>,
        #[case] (permission, target): (&str, &str),
        #[case] expected: PermissionAction,
    ) {
        let mut ruleset = Ruleset::new();
        for (perm, pat, action) in rules {
            ruleset.push(Rule::new(perm, pat, action));
        }
        assert_eq!(ruleset.evaluate(permission, target), expected);
    }

    /// Verifies that wildcards in the first parameter of Rule::new() (permission)
    /// do NOT match other keys.
    ///
    /// The permission field requires exact match only - "*" is treated as a literal
    /// string, not a wildcard. So a rule with permission="*" only matches when the
    /// evaluated permission is exactly "*", not "bash" or any other value.
    ///
    /// This differs from the second parameter (pattern/target) which does support
    /// wildcards.
    #[rstest]
    #[case::star_permission_not_bash(
        vec![("*", "*", PermissionAction::Allow)], // rules: allow all for permission="*" only
        ("bash", "anything"),                      // evaluate: different permission="bash"
        PermissionAction::Deny                     // expected: deny (no match, must be exact)
    )]
    #[case::star_permission_matches_star(
        vec![("*", "*", PermissionAction::Allow)], // rules: allow all for permission="*"
        ("*", "anything"),                         // evaluate: exact match on permission="*"
        PermissionAction::Allow                    // expected: allow (exact permission match)
    )]
    #[case::bash_star_permission_not_bash(
        vec![("bash*", "*", PermissionAction::Allow)], // rules: allow all for permission="bash*"
        ("bash", "anything"),                          // evaluate: different permission="bash"
        PermissionAction::Deny                         // expected: deny (must be exact)
    )]
    #[case::bash_star_permission_not_bash_extended(
        vec![("bash*", "*", PermissionAction::Allow)], // rules: allow all for permission="bash*"
        ("bash-extended", "anything"),                 // evaluate: different permission="bash-extended"
        PermissionAction::Deny                         // expected: deny (must be exact)
    )]
    #[case::bash_star_permission_matches_exact(
        vec![("bash*", "*", PermissionAction::Allow)], // rules: allow all for permission="bash*"
        ("bash*", "anything"),                         // evaluate: exact match on permission="bash*"
        PermissionAction::Allow                        // expected: allow (exact permission match)
    )]
    fn ruleset_evaluate_permission_exact_match_only(
        #[case] rules: Vec<(&str, &str, PermissionAction)>,
        #[case] (permission, target): (&str, &str),
        #[case] expected: PermissionAction,
    ) {
        let mut ruleset = Ruleset::new();
        for (perm, pat, action) in rules {
            ruleset.push(Rule::new(perm, pat, action));
        }
        assert_eq!(ruleset.evaluate(permission, target), expected);
    }

    #[test]
    fn ruleset_is_allowed_convenience() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("bash", "*", PermissionAction::Allow));

        assert!(ruleset.is_allowed("bash", "any"));
        assert!(!ruleset.is_allowed("task", "any"));
    }

    #[test]
    fn ruleset_merge() {
        let mut base = Ruleset::new();
        base.push(Rule::new("bash", "*", PermissionAction::Deny));

        let mut override_rules = Ruleset::new();
        override_rules.push(Rule::new("bash", "*", PermissionAction::Allow));

        base.merge(&override_rules);

        // After merge, the allow rule comes last and wins
        assert_eq!(base.evaluate("bash", "any"), PermissionAction::Allow);
    }

    #[test]
    fn ruleset_merged_multiple() {
        let mut r1 = Ruleset::new();
        r1.push(Rule::new("a", "*", PermissionAction::Deny));

        let mut r2 = Ruleset::new();
        r2.push(Rule::new("a", "*", PermissionAction::Allow));

        let combined = Ruleset::merged([&r1, &r2]);
        assert_eq!(combined.evaluate("a", "x"), PermissionAction::Allow);
    }
}
