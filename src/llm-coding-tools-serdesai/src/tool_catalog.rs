//! Explicit default runtime tool catalog for SerdesAI agent builds.
//!
//! This module provides a cloneable, data-only tool catalog used during JIT agent
//! construction. Each [`ToolCatalogEntry`] pairs a canonical tool name with a
//! [`ToolCatalogKind`] variant that later runtime layers can match on to instantiate
//! concrete tools on demand.
//!
//! The default catalog exposed by [`default_tools()`] covers the non-Task tool surface
//! (read, write, edit, glob, grep, bash, webfetch, todoread, todowrite). Task is
//! intentionally excluded to keep the catalog focused on standard runtime tools.

use llm_coding_tools_core::tool_names;

/// Cloneable metadata for a runtime tool that can be materialized later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    /// Canonical tool name exposed to models.
    pub name: &'static str,
    /// Concrete tool variant used during later runtime instantiation.
    pub kind: ToolCatalogKind,
}

impl ToolCatalogEntry {
    /// Creates a catalog entry from a canonical tool name and concrete kind.
    pub const fn new(name: &'static str, kind: ToolCatalogKind) -> Self {
        Self { name, kind }
    }
}

/// Explicit tool variants supported by the default SerdesAI runtime surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCatalogKind {
    /// Read file contents tool.
    Read,
    /// Write file contents tool.
    Write,
    /// Edit file contents tool.
    Edit,
    /// Glob file pattern matching tool.
    Glob,
    /// Grep text search tool.
    Grep,
    /// Bash command execution tool.
    Bash,
    /// Web fetch tool for HTTP requests.
    WebFetch,
    /// Todo read tool for reading todo items.
    TodoRead,
    /// Todo write tool for creating and updating todo items.
    TodoWrite,
}

const DEFAULT_TOOLS: [ToolCatalogEntry; 9] = [
    ToolCatalogEntry::new(tool_names::READ, ToolCatalogKind::Read),
    ToolCatalogEntry::new(tool_names::WRITE, ToolCatalogKind::Write),
    ToolCatalogEntry::new(tool_names::EDIT, ToolCatalogKind::Edit),
    ToolCatalogEntry::new(tool_names::GLOB, ToolCatalogKind::Glob),
    ToolCatalogEntry::new(tool_names::GREP, ToolCatalogKind::Grep),
    ToolCatalogEntry::new(tool_names::BASH, ToolCatalogKind::Bash),
    ToolCatalogEntry::new(tool_names::WEBFETCH, ToolCatalogKind::WebFetch),
    ToolCatalogEntry::new(tool_names::TODO_READ, ToolCatalogKind::TodoRead),
    ToolCatalogEntry::new(tool_names::TODO_WRITE, ToolCatalogKind::TodoWrite),
];

/// Returns the explicit default non-Task tool catalog for SerdesAI runtimes.
pub fn default_tools() -> Vec<ToolCatalogEntry> {
    // Keep the exported value data-only so later prompts can instantiate tools explicitly.
    DEFAULT_TOOLS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::{default_tools, ToolCatalogEntry, ToolCatalogKind};
    use llm_coding_tools_core::tool_names;
    use std::collections::BTreeSet;

    #[test]
    fn default_tools_match_expected_catalog() {
        assert_eq!(
            default_tools(),
            vec![
                ToolCatalogEntry::new(tool_names::READ, ToolCatalogKind::Read),
                ToolCatalogEntry::new(tool_names::WRITE, ToolCatalogKind::Write),
                ToolCatalogEntry::new(tool_names::EDIT, ToolCatalogKind::Edit),
                ToolCatalogEntry::new(tool_names::GLOB, ToolCatalogKind::Glob),
                ToolCatalogEntry::new(tool_names::GREP, ToolCatalogKind::Grep),
                ToolCatalogEntry::new(tool_names::BASH, ToolCatalogKind::Bash),
                ToolCatalogEntry::new(tool_names::WEBFETCH, ToolCatalogKind::WebFetch),
                ToolCatalogEntry::new(tool_names::TODO_READ, ToolCatalogKind::TodoRead),
                ToolCatalogEntry::new(tool_names::TODO_WRITE, ToolCatalogKind::TodoWrite),
            ],
        );
    }

    #[test]
    fn default_tools_exclude_task_and_keep_names_unique() {
        let tools = default_tools();
        assert!(tools.iter().all(|entry| entry.name != tool_names::TASK));

        let unique_names = tools
            .iter()
            .map(|entry| entry.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_names.len(), tools.len());
    }
}
