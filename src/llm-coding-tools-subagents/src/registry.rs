//! Subagent registry with permission-aware filtering.
//!
//! Provides storage and lookup for agent configurations with support for
//! filtering by mode and permission rules.

use crate::config::{AgentConfig, AgentMode};
use crate::permission::Ruleset;
use std::collections::HashMap;

/// Registry of agent configurations with permission-aware filtering.
///
/// Stores agents by name and provides methods to list, lookup, and filter
/// based on mode and permission rules.
#[derive(Debug, Clone, Default)]
pub struct SubagentRegistry {
    agents: HashMap<String, AgentConfig>,
}

impl SubagentRegistry {
    /// Creates an empty registry.
    #[inline]
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Creates a registry from a map of agent configurations.
    #[inline]
    pub fn from_map(agents: HashMap<String, AgentConfig>) -> Self {
        Self { agents }
    }

    /// Returns the number of registered agents.
    #[inline]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Returns true if the registry is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Inserts an agent configuration.
    ///
    /// Returns the previous configuration if the name was already present.
    #[inline]
    pub fn insert(&mut self, config: AgentConfig) -> Option<AgentConfig> {
        self.agents.insert(config.name.clone(), config)
    }

    /// Gets an agent configuration by name.
    #[inline]
    pub fn get(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    /// Lists all agents matching the given mode filter.
    ///
    /// - [`AgentMode::Primary`]: Returns only primary agents
    /// - [`AgentMode::Subagent`]: Returns only subagents
    /// - [`AgentMode::All`]: Returns all agents
    pub fn list(&self, mode: AgentMode) -> Vec<&AgentConfig> {
        self.agents
            .values()
            .filter(|config| match mode {
                AgentMode::All => true,
                AgentMode::Primary => {
                    matches!(config.mode, AgentMode::Primary | AgentMode::All)
                }
                AgentMode::Subagent => {
                    matches!(config.mode, AgentMode::Subagent | AgentMode::All)
                }
            })
            .collect()
    }

    /// Filters agents accessible to a caller based on their permission rules.
    ///
    /// Returns agents whose names are allowed by the caller's `task` permission.
    /// This is used to determine which subagents a primary agent can invoke.
    ///
    /// **Note:** Only agents with [`AgentMode::Subagent`] or [`AgentMode::All`] are
    /// considered. Primary-only agents are excluded since they cannot be invoked
    /// as subagents.
    ///
    /// # Arguments
    ///
    /// * `caller_rules` - The permission ruleset of the calling agent
    pub fn filter_accessible<'a>(&'a self, caller_rules: &Ruleset) -> Vec<&'a AgentConfig> {
        self.agents
            .values()
            .filter(|config| {
                // Only subagent-capable agents can be invoked via task
                // Exclude AgentMode::Primary (primary-only agents)
                matches!(config.mode, AgentMode::Subagent | AgentMode::All)
                    // Check if caller can invoke this agent via "task" permission
                    && caller_rules.is_allowed("task", &config.name)
            })
            .collect()
    }

    /// Returns only the tool names that are allowed by the given ruleset.
    ///
    /// Convenience wrapper that delegates to [`Ruleset::allowed_tools`].
    /// Each tool is evaluated with `is_allowed(tool_name, "*")` - meaning tools
    /// with only pattern-specific allow rules won't be included unless there's
    /// a `"*"` pattern allow rule for that tool.
    ///
    /// # Arguments
    ///
    /// * `rules` - The permission ruleset to filter against
    /// * `tool_names` - Iterator of tool names to filter
    ///
    /// # Example
    ///
    /// ```
    /// use llm_coding_tools_subagents::{SubagentRegistry, Ruleset, Rule, PermissionAction};
    ///
    /// let registry = SubagentRegistry::new();
    /// let mut rules = Ruleset::new();
    /// rules.push(Rule::new("bash", "*", PermissionAction::Allow));
    /// rules.push(Rule::new("read", "*", PermissionAction::Allow));
    ///
    /// let tools = ["bash", "read", "write", "edit"];
    /// let allowed = registry.allowed_tools(&rules, tools.iter().copied());
    ///
    /// assert_eq!(allowed, vec!["bash".to_string(), "read".to_string()]);
    /// ```
    #[inline]
    pub fn allowed_tools<'a, I>(&self, rules: &Ruleset, tool_names: I) -> Vec<String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        rules.allowed_tools(tool_names)
    }

    /// Returns an iterator over all agent configurations.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&String, &AgentConfig)> {
        self.agents.iter()
    }

    /// Returns an iterator over agent names.
    #[inline]
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.agents.keys()
    }
}

impl FromIterator<AgentConfig> for SubagentRegistry {
    fn from_iter<I: IntoIterator<Item = AgentConfig>>(iter: I) -> Self {
        let agents: HashMap<String, AgentConfig> = iter
            .into_iter()
            .map(|config| (config.name.clone(), config))
            .collect();
        Self { agents }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionAction;
    use crate::permission::Rule;
    use indexmap::IndexMap;

    fn make_agent(name: &str, mode: AgentMode) -> AgentConfig {
        AgentConfig {
            name: name.to_string(),
            mode,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        }
    }

    #[test]
    fn registry_insert_and_get() {
        let mut registry = SubagentRegistry::new();
        let agent = make_agent("test", AgentMode::Subagent);

        assert!(registry.get("test").is_none());
        registry.insert(agent);
        assert!(registry.get("test").is_some());
        assert_eq!(registry.get("test").unwrap().name, "test");
    }

    #[test]
    fn registry_list_primary_only() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("primary1", AgentMode::Primary));
        registry.insert(make_agent("sub1", AgentMode::Subagent));
        registry.insert(make_agent("both1", AgentMode::All));

        let primaries = registry.list(AgentMode::Primary);

        assert_eq!(primaries.len(), 2);
        let names: Vec<_> = primaries.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"primary1"));
        assert!(names.contains(&"both1"));
    }

    #[test]
    fn registry_list_subagent_only() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("primary1", AgentMode::Primary));
        registry.insert(make_agent("sub1", AgentMode::Subagent));
        registry.insert(make_agent("both1", AgentMode::All));

        let subagents = registry.list(AgentMode::Subagent);

        assert_eq!(subagents.len(), 2);
        let names: Vec<_> = subagents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"sub1"));
        assert!(names.contains(&"both1"));
    }

    #[test]
    fn registry_list_all() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("primary1", AgentMode::Primary));
        registry.insert(make_agent("sub1", AgentMode::Subagent));
        registry.insert(make_agent("both1", AgentMode::All));

        let all = registry.list(AgentMode::All);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn registry_filter_accessible_allows_matching_subagents() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("orchestrator-builder", AgentMode::Subagent));
        registry.insert(make_agent("orchestrator-tester", AgentMode::Subagent));
        registry.insert(make_agent("random-agent", AgentMode::Subagent));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));

        let accessible = registry.filter_accessible(&rules);

        assert_eq!(accessible.len(), 2);
        let names: Vec<_> = accessible.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"orchestrator-builder"));
        assert!(names.contains(&"orchestrator-tester"));
        assert!(!names.contains(&"random-agent"));
    }

    #[test]
    fn registry_filter_accessible_excludes_primary_only() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("sub-agent", AgentMode::Subagent));
        registry.insert(make_agent("primary-only", AgentMode::Primary));
        registry.insert(make_agent("both-modes", AgentMode::All));

        // Allow all agents by name
        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));

        let accessible = registry.filter_accessible(&rules);

        // Primary-only agents should be excluded
        assert_eq!(accessible.len(), 2);
        let names: Vec<_> = accessible.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"sub-agent"));
        assert!(names.contains(&"both-modes"));
        assert!(!names.contains(&"primary-only"));
    }

    #[test]
    fn registry_filter_accessible_default_deny() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("agent1", AgentMode::Subagent));

        let rules = Ruleset::new(); // Empty ruleset = default deny

        let accessible = registry.filter_accessible(&rules);
        assert!(accessible.is_empty());
    }

    #[test]
    fn registry_from_iterator() {
        let agents = vec![
            make_agent("a", AgentMode::Subagent),
            make_agent("b", AgentMode::Primary),
        ];

        let registry: SubagentRegistry = agents.into_iter().collect();

        assert_eq!(registry.len(), 2);
        assert!(registry.get("a").is_some());
        assert!(registry.get("b").is_some());
    }

    #[test]
    fn registry_from_map() {
        let mut map = HashMap::new();
        map.insert("test".to_string(), make_agent("test", AgentMode::Subagent));

        let registry = SubagentRegistry::from_map(map);
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registry_allowed_tools_delegates_to_ruleset() {
        let registry = SubagentRegistry::new();

        let mut rules = Ruleset::new();
        rules.push(Rule::new("bash", "*", PermissionAction::Allow));
        rules.push(Rule::new("read", "*", PermissionAction::Allow));
        rules.push(Rule::new("write", "*", PermissionAction::Deny));

        let tools = ["bash", "read", "write", "edit"];
        let allowed = registry.allowed_tools(&rules, tools.iter().copied());

        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"bash".to_string()));
        assert!(allowed.contains(&"read".to_string()));
    }

    #[test]
    fn registry_allowed_tools_uses_wildcard_subject() {
        let registry = SubagentRegistry::new();

        // Rule allows "bash" only for specific subject pattern, not "*"
        let mut rules = Ruleset::new();
        rules.push(Rule::new("bash", "specific-*", PermissionAction::Allow));

        let tools = ["bash"];
        let allowed = registry.allowed_tools(&rules, tools.iter().copied());

        // Should be empty because allowed_tools uses "*" as subject
        assert!(allowed.is_empty());
    }
}
