//! Extension traits for core types.
//!
//! Provides additional constructors and helpers for types from the core crate
//! that depend on agent-specific serialization formats.

use crate::config::PermissionRule;
use indexmap::IndexMap;
use llm_coding_tools_core::permissions::{Rule, Ruleset};

/// Extension trait for building [`Ruleset`] from agent permission configs.
pub trait RulesetExt {
    /// Creates a [`Ruleset`] from frontmatter permission configuration.
    ///
    /// The config maps permission keys to either:
    /// - A direct action (`"allow"` or `"deny"`) applying to pattern `"*"`
    /// - A map of `{ pattern: action }` for per-pattern rules
    ///
    /// Rules are added in iteration order (preserved by [`IndexMap`]).
    ///
    /// # Example
    ///
    /// ```
    /// use llm_coding_tools_agents::{Ruleset, RulesetExt, PermissionRule};
    /// use llm_coding_tools_core::permissions::PermissionAction;
    /// use indexmap::IndexMap;
    ///
    /// let mut config = IndexMap::new();
    /// config.insert(
    ///     "bash".to_string(),
    ///     PermissionRule::Action(PermissionAction::Allow),
    /// );
    ///
    /// let ruleset = Ruleset::from_permission_config(&config);
    /// assert!(ruleset.is_allowed("bash", "*"));
    /// ```
    fn from_permission_config(config: &IndexMap<String, PermissionRule>) -> Self;
}

impl RulesetExt for Ruleset {
    fn from_permission_config(config: &IndexMap<String, PermissionRule>) -> Self {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::permissions::PermissionAction;

    #[test]
    fn from_permission_config_simple_action() {
        let mut config = IndexMap::new();
        config.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );

        let ruleset = Ruleset::from_permission_config(&config);

        assert_eq!(ruleset.len(), 1);
        let rule = ruleset.iter().next().unwrap();
        assert_eq!(rule.permission(), "bash");
        assert_eq!(rule.pattern(), "*");
        assert_eq!(rule.action(), PermissionAction::Allow);
    }

    #[test]
    fn from_permission_config_pattern_map() {
        let mut patterns = IndexMap::new();
        patterns.insert("orchestrator-*".to_string(), PermissionAction::Allow);
        patterns.insert("*".to_string(), PermissionAction::Deny);

        let mut config = IndexMap::new();
        config.insert("task".to_string(), PermissionRule::Pattern(patterns));

        let ruleset = Ruleset::from_permission_config(&config);

        assert_eq!(ruleset.len(), 2);
        let rules: Vec<_> = ruleset.iter().collect();
        assert_eq!(rules[0].permission(), "task");
        assert_eq!(rules[0].pattern(), "orchestrator-*");
        assert_eq!(rules[1].permission(), "task");
        assert_eq!(rules[1].pattern(), "*");
    }
}
