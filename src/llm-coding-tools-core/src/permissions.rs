//! Permission evaluation for tool and delegation access control.
//!
//! This module provides a small, framework-agnostic policy model built from
//! ordered [`Rule`] entries inside a [`Ruleset`].
//!
//! - A rule is `(permission_key, subject_pattern, action)`.
//! - Evaluation is **last-match-wins**.
//! - If nothing matches, the result is [`PermissionAction::Deny`].
//!
//! Matching behavior:
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
pub enum PermissionAction {
    /// Tool is denied.
    #[default]
    Deny,
    /// Tool is allowed.
    Allow,
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
        permission: impl Into<String>,
        pattern: impl Into<String>,
        action: PermissionAction,
    ) -> Self {
        let permission = permission.into();
        let pattern = pattern.into();

        Self {
            permission: PackedPermission::new(hash_u64(&permission), action),
            pattern: TinyString::<[u8; 14]>::from(pattern.as_str()),
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

    /// Returns only the tool names that are allowed by this ruleset.
    ///
    /// Each tool is checked with `is_allowed(tool_name, "*")` - the tool name
    /// as the permission key and `"*"` as the subject.
    ///
    /// **Note:** Because this uses `"*"` as the subject, tools with only
    /// pattern-specific allow rules (e.g., `Rule::new("bash", "specific-*", Allow)`)
    /// won't be included unless there's also a `"*"` pattern allow rule for that tool.
    ///
    /// # Arguments
    ///
    /// * `tool_names` - Iterator of tool names to filter
    pub fn allowed_tools<'a, I>(&self, tool_names: I) -> Vec<String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        tool_names
            .into_iter()
            .filter(|name| self.is_allowed(name, "*"))
            .map(|s| s.to_string())
            .collect()
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

    // ===== Wildcard matching tests =====

    #[test]
    fn wildcard_star_matches_everything() {
        assert!(wildcard_match("anything", "*"));
        assert!(wildcard_match("", "*"));
        assert!(wildcard_match("a/b/c", "*"));
    }

    #[test]
    fn wildcard_exact_match() {
        assert!(wildcard_match("bash", "bash"));
        assert!(!wildcard_match("bash", "task"));
    }

    #[test]
    fn wildcard_prefix_star() {
        assert!(wildcard_match("orchestrator-builder", "orchestrator-*"));
        assert!(wildcard_match("orchestrator-", "orchestrator-*"));
        assert!(!wildcard_match("other-builder", "orchestrator-*"));
    }

    #[test]
    fn wildcard_suffix_star() {
        assert!(wildcard_match("pre-bash", "*-bash"));
        assert!(wildcard_match("-bash", "*-bash"));
        assert!(!wildcard_match("bash-post", "*-bash"));
    }

    #[test]
    fn wildcard_middle_star() {
        assert!(wildcard_match("a-middle-z", "a-*-z"));
        assert!(wildcard_match("a--z", "a-*-z"));
        assert!(!wildcard_match("a-middle", "a-*-z"));
    }

    #[test]
    fn wildcard_question_mark() {
        assert!(wildcard_match("test", "te?t"));
        assert!(wildcard_match("teat", "te?t"));
        assert!(!wildcard_match("teest", "te?t"));
        assert!(!wildcard_match("tet", "te?t"));
    }

    #[test]
    fn wildcard_multiple_stars() {
        assert!(wildcard_match("a/b/c", "*/*"));
        assert!(wildcard_match("abc", "*a*c*"));
    }

    #[test]
    fn wildcard_empty_input() {
        assert!(wildcard_match("", "*"));
        assert!(!wildcard_match("", "?"));
        assert!(!wildcard_match("", "a"));
    }

    // ===== Rule tests =====

    #[test]
    fn rule_preserves_pattern_casing() {
        let rule = Rule::new("BASH", "PATTERN", PermissionAction::Allow);
        assert_eq!(rule.pattern(), "PATTERN");
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

    // ===== Ruleset tests =====

    #[test]
    fn ruleset_evaluate_default_deny() {
        let ruleset = Ruleset::new();
        assert_eq!(ruleset.evaluate("bash", "anything"), PermissionAction::Deny);
    }

    #[test]
    fn ruleset_evaluate_simple_allow() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("bash", "*", PermissionAction::Allow));

        assert_eq!(
            ruleset.evaluate("bash", "anything"),
            PermissionAction::Allow
        );
        assert_eq!(ruleset.evaluate("task", "anything"), PermissionAction::Deny);
    }

    #[test]
    fn ruleset_evaluate_last_match_wins() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
        ruleset.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));

        // "orchestrator-builder" matches both rules, but last one wins
        assert_eq!(
            ruleset.evaluate("task", "orchestrator-builder"),
            PermissionAction::Allow
        );
        // "random-agent" only matches first rule
        assert_eq!(
            ruleset.evaluate("task", "random-agent"),
            PermissionAction::Deny
        );
    }

    #[test]
    fn ruleset_evaluate_case_sensitive() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("BASH", "*", PermissionAction::Allow));

        assert_eq!(ruleset.evaluate("BASH", "test"), PermissionAction::Allow);
        assert_eq!(ruleset.evaluate("bash", "test"), PermissionAction::Deny);
        assert_eq!(ruleset.evaluate("Bash", "test"), PermissionAction::Deny);
    }

    #[test]
    fn ruleset_evaluate_pattern_case_sensitive() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("task", "AGENT-*", PermissionAction::Allow));

        assert_eq!(
            ruleset.evaluate("task", "AGENT-foo"),
            PermissionAction::Allow
        );
        assert_eq!(
            ruleset.evaluate("task", "agent-foo"),
            PermissionAction::Deny
        );
        assert_eq!(
            ruleset.evaluate("task", "Agent-Bar"),
            PermissionAction::Deny
        );
    }

    #[test]
    fn ruleset_evaluate_permission_exact_match_only() {
        // Wildcards in permission key should NOT match multiple tools
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("*", "*", PermissionAction::Allow));

        // A rule with permission "*" only matches permission key "*", not "bash"
        assert_eq!(ruleset.evaluate("bash", "anything"), PermissionAction::Deny);
        assert_eq!(ruleset.evaluate("*", "anything"), PermissionAction::Allow);
    }

    #[test]
    fn ruleset_evaluate_permission_no_wildcard_expansion() {
        // Rule with "bash*" permission should NOT match "bash-extended"
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("bash*", "*", PermissionAction::Allow));

        assert_eq!(ruleset.evaluate("bash", "anything"), PermissionAction::Deny);
        assert_eq!(
            ruleset.evaluate("bash-extended", "anything"),
            PermissionAction::Deny
        );
        assert_eq!(
            ruleset.evaluate("bash*", "anything"),
            PermissionAction::Allow
        );
    }

    #[test]
    fn ruleset_is_allowed_convenience() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("bash", "*", PermissionAction::Allow));

        assert!(ruleset.is_allowed("bash", "any"));
        assert!(!ruleset.is_allowed("task", "any"));
    }

    #[test]
    fn ruleset_allowed_tools_filters_correctly() {
        let mut rules = Ruleset::new();
        rules.push(Rule::new("bash", "*", PermissionAction::Allow));
        rules.push(Rule::new("read", "*", PermissionAction::Allow));
        rules.push(Rule::new("write", "*", PermissionAction::Deny));

        let tools = ["bash", "read", "write", "edit"];
        let allowed = rules.allowed_tools(tools.iter().copied());

        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"bash".to_string()));
        assert!(allowed.contains(&"read".to_string()));
    }

    #[test]
    fn ruleset_allowed_tools_default_deny() {
        let rules = Ruleset::new();
        let tools = ["bash", "read"];
        let allowed = rules.allowed_tools(tools.iter().copied());

        assert!(allowed.is_empty());
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

    #[test]
    fn allowed_tools_preserves_original_casing() {
        let mut rules = Ruleset::new();
        rules.push(Rule::new("Bash", "*", PermissionAction::Allow));
        rules.push(Rule::new("READ", "*", PermissionAction::Allow));

        // Input with mixed case
        let tools = ["Bash", "READ", "Write"];
        let allowed = rules.allowed_tools(tools.iter().copied());

        // Output should preserve original casing
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"Bash".to_string()));
        assert!(allowed.contains(&"READ".to_string()));
    }

    #[test]
    fn ruleset_precedence_specific_overrides_wildcard_when_specific_is_last() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
        ruleset.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));
        assert_eq!(
            ruleset.evaluate("task", "orchestrator-review"),
            PermissionAction::Allow
        );
    }

    #[test]
    fn ruleset_precedence_wildcard_overrides_specific_when_wildcard_is_last() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));
        ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
        assert_eq!(
            ruleset.evaluate("task", "orchestrator-review"),
            PermissionAction::Deny
        );
    }
}
