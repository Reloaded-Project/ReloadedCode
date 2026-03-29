//! # Agent Catalog
//!
//! In-memory store for loaded [`AgentConfig`] entries.
//!
//! ## Guarantees
//! - Keys are resolved agent names.
//! - Later inserts overwrite earlier entries with the same name.
//! - Stores data only (no permission or mode enforcement).

use crate::types::AgentConfig;
use ahash::AHashMap;

/// In-memory catalog of [`AgentConfig`] values loaded by [`crate::AgentLoader`].
///
/// Upstream framework integrations (for example, serdesAI registry builders)
/// consume this catalog to construct runtime agents.
///
/// This type stores configuration data only. It does not apply permission
/// filtering or mode-based access control.
#[derive(Debug, Clone, Default)]
pub struct AgentCatalog {
    agents: AHashMap<String, AgentConfig>,
}

impl AgentCatalog {
    /// Creates an empty [`AgentCatalog`].
    ///
    /// Parameters:
    /// - None.
    ///
    /// Returns:
    /// - A new empty [`AgentCatalog`].
    #[inline]
    pub fn new() -> Self {
        Self {
            agents: AHashMap::new(),
        }
    }

    /// Returns an iterator over all stored [`AgentConfig`] values.
    ///
    /// Parameters:
    /// - None.
    ///
    /// Returns:
    /// - An iterator over borrowed [`AgentConfig`] values.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &AgentConfig> {
        self.agents.values()
    }

    /// Looks up an agent configuration by name.
    ///
    /// Parameters:
    /// - `name`: The resolved agent name.
    ///
    /// Returns:
    /// - [`Option::Some`] with a borrowed [`AgentConfig`] when found.
    /// - [`Option::None`] when no config exists with that name.
    #[inline]
    pub fn by_name(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }

    /// Inserts an agent configuration into the catalog.
    ///
    /// Parameters:
    /// - `config`: The [`AgentConfig`] to insert.
    ///
    /// Returns:
    /// - [`Option::Some`] with the previous [`AgentConfig`] if the name already existed.
    /// - [`Option::None`] if the name was not present.
    pub(crate) fn insert(&mut self, config: AgentConfig) -> Option<AgentConfig> {
        self.agents.insert(config.name.to_string(), config)
    }

    /// Creates a catalog from an iterator of agent configurations.
    ///
    /// Parameters:
    /// - `entries`: The [`AgentConfig`] values to insert.
    ///
    /// Returns:
    /// - A populated [`AgentCatalog`].
    /// - If duplicate names exist, the last entry for each name is retained.
    pub fn from_entries(entries: impl IntoIterator<Item = AgentConfig>) -> Self {
        Self {
            agents: entries
                .into_iter()
                .map(|c| (c.name.to_string(), c))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AgentMode;
    use crate::AgentToolSettings;
    use ahash::AHashMap;
    use indexmap::IndexMap;

    #[test]
    fn catalog_iter_and_by_name() {
        let mut catalog = AgentCatalog::new();
        catalog.insert(AgentConfig {
            name: "alpha".into(),
            mode: AgentMode::Subagent,
            description: Default::default(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: AHashMap::new(),
            tool_settings: AgentToolSettings::default(),
            prompt: Default::default(),
        });
        catalog.insert(AgentConfig {
            name: "beta".into(),
            mode: AgentMode::Subagent,
            description: Default::default(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: AHashMap::new(),
            tool_settings: AgentToolSettings::default(),
            prompt: Default::default(),
        });

        let names: Vec<_> = catalog.iter().map(|config| &*config.name).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(catalog.by_name("beta").is_some());
    }
}
