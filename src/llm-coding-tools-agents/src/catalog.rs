//! Config-only catalog of agent configurations.

use crate::config::AgentConfig;
use std::collections::HashMap;

/// Config-only storage for agent configurations loaded by [`crate::AgentLoader`].
///
/// Stores [`AgentConfig`] entries by name and provides lightweight read access
/// via iterators and name-based lookup. Unlike [`crate::SubagentRegistry`], the catalog
/// does not perform permission filtering or mode-based access control.
///
/// The catalog is intended for framework registries to iterate and build
/// native agents from loaded configurations.
#[derive(Debug, Clone, Default)]
pub struct AgentCatalog {
    agents: HashMap<String, AgentConfig>,
}

impl AgentCatalog {
    /// Creates an empty catalog of agent configs.
    ///
    /// Returns: a new [`AgentCatalog`].
    #[inline]
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Returns an iterator over all stored agent configs.
    ///
    /// Returns: an iterator of borrowed [`AgentConfig`] entries.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &AgentConfig> {
        self.agents.values()
    }

    /// Looks up an agent configuration by name.
    ///
    /// Parameters:
    /// - `name`: the derived or frontmatter agent name.
    ///
    /// Returns: `Some(&AgentConfig)` when found, otherwise `None`.
    #[inline]
    pub fn by_name(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    /// Inserts an agent configuration into the catalog.
    ///
    /// Returns the previous configuration if the name was already present.
    pub(crate) fn insert(&mut self, config: AgentConfig) -> Option<AgentConfig> {
        self.agents.insert(config.name.clone(), config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentMode;
    use indexmap::IndexMap;
    use std::collections::HashMap;

    #[test]
    fn catalog_iter_and_by_name() {
        let mut catalog = AgentCatalog::new();
        catalog.insert(AgentConfig {
            name: "alpha".to_string(),
            mode: AgentMode::Subagent,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        });
        catalog.insert(AgentConfig {
            name: "beta".to_string(),
            mode: AgentMode::Subagent,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        });

        let names: Vec<_> = catalog.iter().map(|config| config.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(catalog.by_name("beta").is_some());
    }
}
