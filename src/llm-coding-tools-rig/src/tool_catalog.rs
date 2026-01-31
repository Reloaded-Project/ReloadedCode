//! Cloneable tool catalog for rig framework.
//!
//! Provides [`ToolCatalogEntry`] enum that wraps all tool types with
//! [`Clone`] support for registry-based agent construction.
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_rig::{default_tools, TodoState, ToolCatalogEntry};
//! use llm_coding_tools_core::path::AllowedPathResolver;
//!
//! // Get default tools with line numbers and absolute paths
//! let tools = default_tools(true, None, TodoState::new());
//!
//! // Get default tools with allowed path sandboxing
//! let resolver = AllowedPathResolver::new(["/allowed/path"]).unwrap();
//! let tools = default_tools(true, Some(resolver), TodoState::new());
//! ```

use crate::absolute;
use crate::allowed;
use crate::{BashTool, TodoReadTool, TodoState, TodoWriteTool, WebFetchTool};
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::ToolContext;

/// Cloneable catalog entry for rig tool instances.
///
/// Provides the tool's name and context for registries and can register the
/// wrapped tool on rig builders.
#[derive(Debug, Clone)]
pub enum ToolCatalogEntry {
    /// Read tool with line numbers enabled (absolute path variant).
    ReadLines(absolute::ReadTool<true>),
    /// Read tool without line numbers (absolute path variant).
    ReadRaw(absolute::ReadTool<false>),
    /// Read tool with line numbers enabled (allowed path variant).
    ReadAllowedLines(allowed::ReadTool<true>),
    /// Read tool without line numbers (allowed path variant).
    ReadAllowedRaw(allowed::ReadTool<false>),
    /// Write tool (absolute path variant).
    Write(absolute::WriteTool),
    /// Write tool (allowed path variant).
    WriteAllowed(allowed::WriteTool),
    /// Edit tool (absolute path variant).
    Edit(absolute::EditTool),
    /// Edit tool (allowed path variant).
    EditAllowed(allowed::EditTool),
    /// Glob tool (absolute path variant).
    Glob(absolute::GlobTool),
    /// Glob tool (allowed path variant).
    GlobAllowed(allowed::GlobTool),
    /// Grep tool with line numbers enabled (absolute path variant).
    GrepLines(absolute::GrepTool<true>),
    /// Grep tool without line numbers (absolute path variant).
    GrepRaw(absolute::GrepTool<false>),
    /// Grep tool with line numbers enabled (allowed path variant).
    GrepAllowedLines(allowed::GrepTool<true>),
    /// Grep tool without line numbers (allowed path variant).
    GrepAllowedRaw(allowed::GrepTool<false>),
    /// Bash shell command execution tool.
    Bash(BashTool),
    /// Web content fetching tool.
    WebFetch(WebFetchTool),
    /// Todo list read tool.
    TodoRead(TodoReadTool),
    /// Todo list write tool.
    TodoWrite(TodoWriteTool),
}

impl ToolCatalogEntry {
    /// Returns the canonical tool name.
    ///
    /// Returns: one of the [`tool_names`] constants (e.g., [`tool_names::READ`]).
    #[inline]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ReadLines(_)
            | Self::ReadRaw(_)
            | Self::ReadAllowedLines(_)
            | Self::ReadAllowedRaw(_) => tool_names::READ,
            Self::Write(_) | Self::WriteAllowed(_) => tool_names::WRITE,
            Self::Edit(_) | Self::EditAllowed(_) => tool_names::EDIT,
            Self::Glob(_) | Self::GlobAllowed(_) => tool_names::GLOB,
            Self::GrepLines(_)
            | Self::GrepRaw(_)
            | Self::GrepAllowedLines(_)
            | Self::GrepAllowedRaw(_) => tool_names::GREP,
            Self::Bash(_) => tool_names::BASH,
            Self::WebFetch(_) => tool_names::WEBFETCH,
            Self::TodoRead(_) => tool_names::TODO_READ,
            Self::TodoWrite(_) => tool_names::TODO_WRITE,
        }
    }

    /// Returns the tool's system prompt context string.
    ///
    /// Returns: the context string for this tool.
    #[inline]
    pub fn context(&self) -> &'static str {
        match self {
            Self::ReadLines(tool) => tool.context(),
            Self::ReadRaw(tool) => tool.context(),
            Self::ReadAllowedLines(tool) => tool.context(),
            Self::ReadAllowedRaw(tool) => tool.context(),
            Self::Write(tool) => tool.context(),
            Self::WriteAllowed(tool) => tool.context(),
            Self::Edit(tool) => tool.context(),
            Self::EditAllowed(tool) => tool.context(),
            Self::Glob(tool) => tool.context(),
            Self::GlobAllowed(tool) => tool.context(),
            Self::GrepLines(tool) => tool.context(),
            Self::GrepRaw(tool) => tool.context(),
            Self::GrepAllowedLines(tool) => tool.context(),
            Self::GrepAllowedRaw(tool) => tool.context(),
            Self::Bash(tool) => tool.context(),
            Self::WebFetch(tool) => tool.context(),
            Self::TodoRead(tool) => tool.context(),
            Self::TodoWrite(tool) => tool.context(),
        }
    }

    /// Registers this tool on a fresh rig agent builder.
    ///
    /// Parameters:
    /// - `builder`: the initial rig agent builder (pre-tool).
    ///
    /// Returns: the builder after registering this tool.
    pub fn register_on<M>(
        self,
        builder: rig::agent::AgentBuilder<M>,
    ) -> rig::agent::AgentBuilderSimple<M>
    where
        M: rig::completion::CompletionModel,
    {
        match self {
            Self::ReadLines(tool) => builder.tool(tool),
            Self::ReadRaw(tool) => builder.tool(tool),
            Self::ReadAllowedLines(tool) => builder.tool(tool),
            Self::ReadAllowedRaw(tool) => builder.tool(tool),
            Self::Write(tool) => builder.tool(tool),
            Self::WriteAllowed(tool) => builder.tool(tool),
            Self::Edit(tool) => builder.tool(tool),
            Self::EditAllowed(tool) => builder.tool(tool),
            Self::Glob(tool) => builder.tool(tool),
            Self::GlobAllowed(tool) => builder.tool(tool),
            Self::GrepLines(tool) => builder.tool(tool),
            Self::GrepRaw(tool) => builder.tool(tool),
            Self::GrepAllowedLines(tool) => builder.tool(tool),
            Self::GrepAllowedRaw(tool) => builder.tool(tool),
            Self::Bash(tool) => builder.tool(tool),
            Self::WebFetch(tool) => builder.tool(tool),
            Self::TodoRead(tool) => builder.tool(tool),
            Self::TodoWrite(tool) => builder.tool(tool),
        }
    }

    /// Registers this tool on an existing rig agent builder.
    ///
    /// Parameters:
    /// - `builder`: the rig builder after at least one tool has been registered.
    ///
    /// Returns: the builder after registering this tool.
    pub fn register_on_simple<M>(
        self,
        builder: rig::agent::AgentBuilderSimple<M>,
    ) -> rig::agent::AgentBuilderSimple<M>
    where
        M: rig::completion::CompletionModel,
    {
        match self {
            Self::ReadLines(tool) => builder.tool(tool),
            Self::ReadRaw(tool) => builder.tool(tool),
            Self::ReadAllowedLines(tool) => builder.tool(tool),
            Self::ReadAllowedRaw(tool) => builder.tool(tool),
            Self::Write(tool) => builder.tool(tool),
            Self::WriteAllowed(tool) => builder.tool(tool),
            Self::Edit(tool) => builder.tool(tool),
            Self::EditAllowed(tool) => builder.tool(tool),
            Self::Glob(tool) => builder.tool(tool),
            Self::GlobAllowed(tool) => builder.tool(tool),
            Self::GrepLines(tool) => builder.tool(tool),
            Self::GrepRaw(tool) => builder.tool(tool),
            Self::GrepAllowedLines(tool) => builder.tool(tool),
            Self::GrepAllowedRaw(tool) => builder.tool(tool),
            Self::Bash(tool) => builder.tool(tool),
            Self::WebFetch(tool) => builder.tool(tool),
            Self::TodoRead(tool) => builder.tool(tool),
            Self::TodoWrite(tool) => builder.tool(tool),
        }
    }
}

/// Builds the default tool catalog for rig.
///
/// Parameters:
/// - `line_numbers`: whether read/grep tools include line numbers in output.
/// - `resolver`: `None` for absolute tools, or `Some(resolver)` for allowed tools.
/// - `todo_state`: shared [`TodoState`] used by todo read/write tools.
///
/// Returns: a list of non-Task tool catalog entries in canonical order.
pub fn default_tools(
    line_numbers: bool,
    resolver: Option<AllowedPathResolver>,
    todo_state: TodoState,
) -> Vec<ToolCatalogEntry> {
    let mut tools = Vec::with_capacity(9);

    let allowed_resolvers = resolver.map(|resolver| {
        let [read_resolver, write_resolver, edit_resolver, glob_resolver, grep_resolver] =
            [(); 5].map(|_| resolver.clone());
        (
            read_resolver,
            write_resolver,
            edit_resolver,
            glob_resolver,
            grep_resolver,
        )
    });

    match allowed_resolvers {
        None => {
            let read = if line_numbers {
                ToolCatalogEntry::ReadLines(absolute::ReadTool::<true>::new())
            } else {
                ToolCatalogEntry::ReadRaw(absolute::ReadTool::<false>::new())
            };
            let grep = if line_numbers {
                ToolCatalogEntry::GrepLines(absolute::GrepTool::<true>::new())
            } else {
                ToolCatalogEntry::GrepRaw(absolute::GrepTool::<false>::new())
            };

            tools.extend([
                read,
                ToolCatalogEntry::Write(absolute::WriteTool::new()),
                ToolCatalogEntry::Edit(absolute::EditTool::new()),
                ToolCatalogEntry::Glob(absolute::GlobTool::new()),
                grep,
            ]);
        }
        Some((read_resolver, write_resolver, edit_resolver, glob_resolver, grep_resolver)) => {
            let read = if line_numbers {
                ToolCatalogEntry::ReadAllowedLines(allowed::ReadTool::<true>::new(read_resolver))
            } else {
                ToolCatalogEntry::ReadAllowedRaw(allowed::ReadTool::<false>::new(read_resolver))
            };
            let grep = if line_numbers {
                ToolCatalogEntry::GrepAllowedLines(allowed::GrepTool::<true>::new(grep_resolver))
            } else {
                ToolCatalogEntry::GrepAllowedRaw(allowed::GrepTool::<false>::new(grep_resolver))
            };

            tools.extend([
                read,
                ToolCatalogEntry::WriteAllowed(allowed::WriteTool::new(write_resolver)),
                ToolCatalogEntry::EditAllowed(allowed::EditTool::new(edit_resolver)),
                ToolCatalogEntry::GlobAllowed(allowed::GlobTool::new(glob_resolver)),
                grep,
            ]);
        }
    }

    let todo_read = ToolCatalogEntry::TodoRead(TodoReadTool::new(todo_state.clone()));
    let todo_write = ToolCatalogEntry::TodoWrite(TodoWriteTool::new(todo_state));
    tools.extend([
        ToolCatalogEntry::Bash(BashTool::new()),
        ToolCatalogEntry::WebFetch(WebFetchTool::new()),
        todo_read,
        todo_write,
    ]);

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::tool_names;
    use tempfile::TempDir;

    const EXPECTED_NAMES: [&str; 9] = [
        tool_names::READ,
        tool_names::WRITE,
        tool_names::EDIT,
        tool_names::GLOB,
        tool_names::GREP,
        tool_names::BASH,
        tool_names::WEBFETCH,
        tool_names::TODO_READ,
        tool_names::TODO_WRITE,
    ];

    fn assert_default_tools(
        line_numbers: bool,
        resolver: Option<AllowedPathResolver>,
        todo_state: TodoState,
    ) {
        let tools = default_tools(line_numbers, resolver, todo_state);
        let mut names: Vec<_> = tools.iter().map(|tool| tool.name()).collect();
        let mut expected: Vec<_> = EXPECTED_NAMES.into();
        names.sort_unstable();
        expected.sort_unstable();
        assert_eq!(names, expected);
        let mut dedup = names.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), names.len());
        assert!(!names.contains(&tool_names::TASK));
    }

    #[test]
    fn default_tools_absolute_has_unique_names() {
        assert_default_tools(true, None, TodoState::new());
        assert_default_tools(false, None, TodoState::new());
    }

    #[test]
    fn default_tools_allowed_has_unique_names() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        assert_default_tools(true, Some(resolver.clone()), TodoState::new());
        assert_default_tools(false, Some(resolver), TodoState::new());
    }
}
