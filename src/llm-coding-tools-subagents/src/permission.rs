//! Permission evaluation with wildcard pattern matching.
//!
//! Implements a last-match-wins rule evaluation system for tool and subagent access control.

use crate::config::{PermissionAction, PermissionRule};
use indexmap::IndexMap;

/// A single permission rule with pattern-based matching.
///
/// Fields are private to enforce the lowercasing invariant. Use [`Rule::new`] to create
/// rules, which normalizes permission and pattern to lowercase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Permission key (tool name), normalized to lowercase.
    permission: String,
    /// Pattern to match against (e.g., "*", "orchestrator-*"), normalized to lowercase.
    pattern: String,
    /// Action to take when matched.
    action: PermissionAction,
}

impl Rule {
    /// Creates a new rule with normalized (lowercase) permission and pattern.
    #[inline]
    pub fn new(
        permission: impl Into<String>,
        pattern: impl Into<String>,
        action: PermissionAction,
    ) -> Self {
        Self {
            permission: permission.into().to_ascii_lowercase(),
            pattern: pattern.into().to_ascii_lowercase(),
            action,
        }
    }

    /// Returns the permission key (tool name), already normalized to lowercase.
    #[inline]
    pub fn permission(&self) -> &str {
        &self.permission
    }

    /// Returns the pattern, already normalized to lowercase.
    #[inline]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Returns the action for this rule.
    #[inline]
    pub fn action(&self) -> PermissionAction {
        self.action
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

    /// Converts a frontmatter permission config into a [`Ruleset`].
    ///
    /// The config maps permission keys to either:
    /// - A direct action (`"allow"` or `"deny"`) applying to pattern `"*"`
    /// - A map of `{ pattern: action }` for per-pattern rules
    ///
    /// Rules are added in iteration order (preserved by [`IndexMap`]).
    /// Permission keys and patterns are normalized to lowercase.
    pub fn from_config(config: &IndexMap<String, PermissionRule>) -> Self {
        // Estimate capacity: most entries have 1-2 rules
        let mut ruleset = Self::with_capacity(config.len() * 2);

        for (key, rule) in config {
            match rule {
                PermissionRule::Action(action) => {
                    ruleset.push(Rule::new(key, "*", *action));
                }
                PermissionRule::Pattern(patterns) => {
                    for (pattern, action) in patterns {
                        ruleset.push(Rule::new(key, pattern, *action));
                    }
                }
            }
        }

        ruleset
    }

    /// Evaluates the ruleset for a given permission and subject.
    ///
    /// Returns the action from the last matching rule, or [`PermissionAction::Deny`]
    /// if no rule matches (default deny).
    ///
    /// Permission keys are matched with **exact equality** (after lowercasing).
    /// Patterns are matched against subjects using wildcard matching.
    ///
    /// # Arguments
    ///
    /// * `permission` - The permission key (tool name) to check (exact match)
    /// * `subject` - The subject to match against rule patterns (e.g., agent name, path)
    pub fn evaluate(&self, permission: &str, subject: &str) -> PermissionAction {
        let permission_lower = permission.to_ascii_lowercase();
        let subject_lower = subject.to_ascii_lowercase();

        // Last-match-wins: iterate forward, keep overwriting result
        let mut result = PermissionAction::Deny;

        for rule in &self.rules {
            // Permission key: exact match only (no wildcards)
            // Pattern: wildcard match against subject
            if rule.permission == permission_lower && wildcard_match(&subject_lower, &rule.pattern)
            {
                result = rule.action;
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
/// Both inputs should be pre-normalized to lowercase for case-insensitive matching.
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
            // Backtrack: try matching one more character with star
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
    fn rule_normalizes_to_lowercase() {
        let rule = Rule::new("BASH", "PATTERN", PermissionAction::Allow);
        assert_eq!(rule.permission(), "bash");
        assert_eq!(rule.pattern(), "pattern");
    }

    #[test]
    fn rule_getters_return_correct_values() {
        let rule = Rule::new("task", "orchestrator-*", PermissionAction::Allow);
        assert_eq!(rule.permission(), "task");
        assert_eq!(rule.pattern(), "orchestrator-*");
        assert_eq!(rule.action(), PermissionAction::Allow);
    }

    // ===== Ruleset tests =====

    #[test]
    fn ruleset_from_config_simple_action() {
        let mut config = IndexMap::new();
        config.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );

        let ruleset = Ruleset::from_config(&config);

        assert_eq!(ruleset.len(), 1);
        let rule = ruleset.iter().next().unwrap();
        assert_eq!(rule.permission(), "bash");
        assert_eq!(rule.pattern(), "*");
        assert_eq!(rule.action(), PermissionAction::Allow);
    }

    #[test]
    fn ruleset_from_config_pattern_map() {
        let mut patterns = IndexMap::new();
        patterns.insert("orchestrator-*".to_string(), PermissionAction::Allow);
        patterns.insert("*".to_string(), PermissionAction::Deny);

        let mut config = IndexMap::new();
        config.insert("task".to_string(), PermissionRule::Pattern(patterns));

        let ruleset = Ruleset::from_config(&config);

        assert_eq!(ruleset.len(), 2);
        let rules: Vec<_> = ruleset.iter().collect();
        assert_eq!(rules[0].permission(), "task");
        assert_eq!(rules[0].pattern(), "orchestrator-*");
        assert_eq!(rules[1].permission(), "task");
        assert_eq!(rules[1].pattern(), "*");
    }

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
    fn ruleset_evaluate_case_insensitive() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("BASH", "*", PermissionAction::Allow));

        assert_eq!(ruleset.evaluate("bash", "test"), PermissionAction::Allow);
        assert_eq!(ruleset.evaluate("Bash", "test"), PermissionAction::Allow);
        assert_eq!(ruleset.evaluate("BASH", "test"), PermissionAction::Allow);
    }

    #[test]
    fn ruleset_evaluate_pattern_case_insensitive() {
        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("task", "AGENT-*", PermissionAction::Allow));

        assert_eq!(
            ruleset.evaluate("task", "agent-foo"),
            PermissionAction::Allow
        );
        assert_eq!(
            ruleset.evaluate("task", "Agent-Bar"),
            PermissionAction::Allow
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
        rules.push(Rule::new("bash", "*", PermissionAction::Allow));
        rules.push(Rule::new("read", "*", PermissionAction::Allow));

        // Input with mixed case
        let tools = ["Bash", "READ", "Write"];
        let allowed = rules.allowed_tools(tools.iter().copied());

        // Output should preserve original casing
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"Bash".to_string())); // Not "bash"
        assert!(allowed.contains(&"READ".to_string())); // Not "read"
    }
}
