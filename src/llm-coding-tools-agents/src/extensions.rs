//! # Ruleset Extensions
//!
//! Helpers for converting agent permission config into runtime [`Ruleset`] values.
//!
//! ## What This Module Provides
//! - [`RulesetExt`] trait for building a [`Ruleset`] from frontmatter data.
//! - Support for scalar (`allow`/`deny`) and pattern-map permission rules.
//! - Iteration-order preservation via [`IndexMap`] (important for precedence).

use crate::types::PermissionRule;
use indexmap::IndexMap;
use llm_coding_tools_core::permissions::{ExpandError, Rule, Ruleset};

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
    /// use llm_coding_tools_agents::{RulesetExt, PermissionRule};
    /// use llm_coding_tools_core::permissions::{PermissionAction, Ruleset};
    /// use indexmap::IndexMap;
    ///
    /// let mut config = IndexMap::new();
    /// config.insert(
    ///     "bash".to_string(),
    ///     PermissionRule::Action(PermissionAction::Allow),
    /// );
    ///
    /// let ruleset = Ruleset::from_permission_config(&config).unwrap();
    /// assert!(ruleset.is_allowed("bash", "*"));
    /// ```
    fn from_permission_config(
        config: &IndexMap<String, PermissionRule>,
    ) -> Result<Ruleset, ExpandError>;
}

impl RulesetExt for Ruleset {
    /// # Errors
    /// - Returns [`ExpandError`] when a permission pattern is invalid (contains `:`, `//`, or empty segments).
    fn from_permission_config(
        config: &IndexMap<String, PermissionRule>,
    ) -> Result<Ruleset, ExpandError> {
        let mut ruleset = Ruleset::with_capacity(config.len() * 2);

        for (key, rule) in config {
            match rule {
                PermissionRule::Action(action) => {
                    ruleset.push(Rule::new(key.as_str(), "*", *action)?);
                }
                PermissionRule::Pattern(patterns) => {
                    for (pattern, action) in patterns {
                        ruleset.push(Rule::new(key.as_str(), pattern.as_str(), *action)?);
                    }
                }
            }
        }

        Ok(ruleset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::permissions::PermissionAction;

    type TestResult = Result<(), ExpandError>;

    #[test]
    fn from_permission_config_simple_action() -> TestResult {
        let mut config = IndexMap::new();
        config.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );

        let ruleset = Ruleset::from_permission_config(&config)?;

        assert_eq!(ruleset.len(), 1);
        assert!(ruleset.is_allowed("bash", "*"));
        assert!(!ruleset.is_allowed("task", "*"));
        Ok(())
    }

    #[test]
    fn from_permission_config_pattern_map() -> TestResult {
        let mut patterns = IndexMap::new();
        patterns.insert("*".to_string(), PermissionAction::Deny);
        patterns.insert("orchestrator-*".to_string(), PermissionAction::Allow);

        let mut config = IndexMap::new();
        config.insert("task".to_string(), PermissionRule::Pattern(patterns));

        let ruleset = Ruleset::from_permission_config(&config)?;

        assert_eq!(ruleset.len(), 2);
        assert_eq!(
            ruleset.evaluate("task", "orchestrator-builder"),
            PermissionAction::Allow
        );
        assert_eq!(
            ruleset.evaluate("task", "other-agent"),
            PermissionAction::Deny
        );
        Ok(())
    }

    /// Verifies that wildcard permission keys in config are correctly converted
    /// to rules and match various permissions at runtime.
    #[test]
    fn from_permission_config_wildcard_permission_key() -> TestResult {
        let mut config = IndexMap::new();
        // "*": allow - wildcard permission key matches any tool
        config.insert(
            "*".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );
        // "bash": deny - specific tool denied (should override via last-match-wins if ordered after)
        config.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );

        let ruleset = Ruleset::from_permission_config(&config)?;

        // Rules are added in IndexMap iteration order (preserved)
        // Rule 0: ("*", "*", Allow) - wildcard
        // Rule 1: ("bash", "*", Deny) - exact

        // "*" matches any permission key
        assert!(
            ruleset.is_allowed("read", "*"),
            "wildcard should allow read"
        );
        assert!(
            ruleset.is_allowed("task", "*"),
            "wildcard should allow task"
        );

        // "bash" exact rule comes last and wins over "*" wildcard
        assert!(
            !ruleset.is_allowed("bash", "*"),
            "exact bash deny should win"
        );

        // Verify wildcard actually works (reverse order)
        let mut config2 = IndexMap::new();
        config2.insert(
            "bash".to_string(),
            PermissionRule::Action(PermissionAction::Deny),
        );
        config2.insert(
            "*".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );
        let ruleset2 = Ruleset::from_permission_config(&config2)?;
        // Now "*" comes last and wins, so bash is allowed
        assert!(
            ruleset2.is_allowed("bash", "*"),
            "wildcard last should allow bash"
        );
        Ok(())
    }
}
