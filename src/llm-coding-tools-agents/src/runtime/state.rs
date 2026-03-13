//! Holds your loaded agents, default settings, and available tools.
//!
//! ## Public API
//!
//! - [`AgentRuntime`] — Container for loaded agents, defaults, and tools.
//! - [`AgentDefaults`] — Fallback settings when an agent doesn't specify them.

use super::tool_catalog::ToolCatalogEntry;
use crate::AgentCatalog;

/// Default settings used when an agent doesn't specify them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentDefaults {
    /// Default model in `provider/model-id` format.
    pub model: Option<Box<str>>,
    /// Default sampling temperature.
    pub temperature: Option<f32>,
    /// Default nucleus sampling top-p.
    pub top_p: Option<f32>,
}

/// Your loaded agents plus their default settings and available tools.
#[derive(Debug, Clone)]
pub struct AgentRuntime {
    catalog: AgentCatalog,
    defaults: AgentDefaults,
    tools: Vec<ToolCatalogEntry>,
}

impl AgentRuntime {
    #[inline]
    pub(super) fn from_parts(
        catalog: AgentCatalog,
        defaults: AgentDefaults,
        tools: Vec<ToolCatalogEntry>,
    ) -> Self {
        Self {
            catalog,
            defaults,
            tools,
        }
    }

    /// Returns the loaded agent definitions.
    #[inline]
    pub fn catalog(&self) -> &AgentCatalog {
        &self.catalog
    }

    /// Returns the default settings.
    #[inline]
    pub fn defaults(&self) -> &AgentDefaults {
        &self.defaults
    }

    /// Returns the tools available to agents.
    #[inline]
    pub fn tools(&self) -> &[ToolCatalogEntry] {
        &self.tools
    }
}
