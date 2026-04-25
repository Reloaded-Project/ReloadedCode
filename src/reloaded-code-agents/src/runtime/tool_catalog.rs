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
//! todowrite, task.

use reloaded_code_core::tool_metadata::{
    bash as bash_meta, edit as edit_meta, glob as glob_meta, grep as grep_meta, read as read_meta,
    task as task_meta, todo_read as todo_read_meta, todo_write as todo_write_meta,
    webfetch as webfetch_meta, write as write_meta,
};

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
    /// Delegate to subagent via Task tool.
    Task,
}

const DEFAULT_TOOLS: [ToolCatalogEntry; 10] = [
    ToolCatalogEntry::new(read_meta::NAME, ToolCatalogKind::Read),
    ToolCatalogEntry::new(write_meta::NAME, ToolCatalogKind::Write),
    ToolCatalogEntry::new(edit_meta::NAME, ToolCatalogKind::Edit),
    ToolCatalogEntry::new(glob_meta::NAME, ToolCatalogKind::Glob),
    ToolCatalogEntry::new(grep_meta::NAME, ToolCatalogKind::Grep),
    ToolCatalogEntry::new(bash_meta::NAME, ToolCatalogKind::Bash),
    ToolCatalogEntry::new(webfetch_meta::NAME, ToolCatalogKind::WebFetch),
    ToolCatalogEntry::new(todo_read_meta::NAME, ToolCatalogKind::TodoRead),
    ToolCatalogEntry::new(todo_write_meta::NAME, ToolCatalogKind::TodoWrite),
    ToolCatalogEntry::new(task_meta::NAME, ToolCatalogKind::Task),
];

/// Returns the standard tool set.
pub fn default_tools() -> Vec<ToolCatalogEntry> {
    DEFAULT_TOOLS.to_vec()
}

#[cfg(test)]
mod tests {
    use super::{default_tools, ToolCatalogEntry, ToolCatalogKind};
    use reloaded_code_core::tool_metadata::{
        bash as bash_meta, edit as edit_meta, glob as glob_meta, grep as grep_meta,
        read as read_meta, task as task_meta, todo_read as todo_read_meta,
        todo_write as todo_write_meta, webfetch as webfetch_meta, write as write_meta,
    };

    #[test]
    fn default_tools_match_expected_catalog() {
        assert_eq!(
            default_tools(),
            vec![
                ToolCatalogEntry::new(read_meta::NAME, ToolCatalogKind::Read),
                ToolCatalogEntry::new(write_meta::NAME, ToolCatalogKind::Write),
                ToolCatalogEntry::new(edit_meta::NAME, ToolCatalogKind::Edit),
                ToolCatalogEntry::new(glob_meta::NAME, ToolCatalogKind::Glob),
                ToolCatalogEntry::new(grep_meta::NAME, ToolCatalogKind::Grep),
                ToolCatalogEntry::new(bash_meta::NAME, ToolCatalogKind::Bash),
                ToolCatalogEntry::new(webfetch_meta::NAME, ToolCatalogKind::WebFetch,),
                ToolCatalogEntry::new(todo_read_meta::NAME, ToolCatalogKind::TodoRead,),
                ToolCatalogEntry::new(todo_write_meta::NAME, ToolCatalogKind::TodoWrite,),
                ToolCatalogEntry::new(task_meta::NAME, ToolCatalogKind::Task),
            ],
        );
    }
}
