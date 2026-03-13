//! Lists which tools your agents can use.
//!
//! Each [`ToolCatalogEntry`] pairs a tool name with its type ([`ToolCatalogKind`]).
//!
//! # Public API
//!
//! - [`ToolCatalogEntry`] - One tool the runtime can provide to agents
//! - [`ToolCatalogKind`] - The tools your agents can use
//! - [`default_tools()`] - The standard tool set
//!
//! The default tools are: read, write, edit, glob, grep, bash, webfetch, todoread,
//! todowrite. The "task" tool is excluded since it's handled separately.

use llm_coding_tools_core::tool_names;

/// One tool the runtime can provide to agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCatalogEntry {
    /// Tool name exposed to models.
    pub name: &'static str,
    /// Which tool this is.
    pub kind: ToolCatalogKind,
}

impl ToolCatalogEntry {
    /// Creates a tool entry from its name and kind.
    pub const fn new(name: &'static str, kind: ToolCatalogKind) -> Self {
        Self { name, kind }
    }
}

/// The tools your agents can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolCatalogKind {
    /// Read file contents.
    Read,
    /// Write file contents.
    Write,
    /// Edit file contents.
    Edit,
    /// Glob file pattern matching.
    Glob,
    /// Grep text search.
    Grep,
    /// Bash command execution.
    Bash,
    /// Web fetch for HTTP requests.
    WebFetch,
    /// Read todo items.
    TodoRead,
    /// Create and update todo items.
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

/// Returns the standard tool set.
pub fn default_tools() -> Vec<ToolCatalogEntry> {
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
