//! Prompt helpers for built-in tools.
//!
//! This module defines the prompt variants used by built-in tools, tracks which
//! tools are present, and hands off the actual text writing to smaller helper
//! modules.
//!
//! # Public API
//! - [`PathMode`] says whether a tool uses absolute paths or allowed
//!   directories.
//! - [`ToolPrompt`] says which built-in guidance block to render.

use core::fmt::Write as _;

mod common_rules;
mod tool_sections;

/// Heading used for the shared rule block.
pub(crate) const COMMON_RULES_HEADER: &str = "## Common Rules\n";

/// Largest common-rules section length, including [`COMMON_RULES_HEADER`].
pub(crate) const COMMON_RULES_SECTION_MAX_SIZE: usize = 493;

/// Describes how a tool accepts paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    /// The tool accepts absolute filesystem paths.
    Absolute,
    /// The tool accepts paths within allowed directories.
    Allowed,
}

/// Describes the guidance to render for one tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPrompt {
    /// Uses a fixed guidance string as-is.
    Static(&'static str),
    /// Writes guidance for `bash`.
    Bash {
        /// Whether network access is disabled for the bash execution.
        ///
        /// When `true`, the rendered prompt includes a note that network access
        /// is disabled inside the sandbox. This is only meaningful when
        /// `sandboxed` is also `true` - a host-level bash session cannot
        /// restrict networking, so the default is `false`.
        network_disabled: bool,
        /// Whether the bash execution is confined to a Linux sandbox (e.g. bubblewrap).
        ///
        /// When `true`, the rendered prompt notes that commands run inside a Linux
        /// sandbox. Defaults to `false` (unrestricted host execution). Can be
        /// combined with `network_disabled`; setting `network_disabled` without
        /// `sandboxed` has no effect.
        sandboxed: bool,
    },
    /// Writes guidance for `read`.
    Read {
        path_mode: PathMode,
        line_numbers: bool,
    },
    /// Writes guidance for `write`.
    Write { path_mode: PathMode },
    /// Writes guidance for `edit`.
    Edit { path_mode: PathMode },
    /// Writes guidance for `glob`.
    Glob { path_mode: PathMode },
    /// Writes guidance for `grep`.
    Grep {
        path_mode: PathMode,
        line_numbers: bool,
    },
    /// Writes guidance for `webfetch`.
    WebFetch,
    /// Writes guidance for `todoread`.
    TodoRead,
    /// Writes guidance for `todowrite`.
    TodoWrite,
    /// Writes guidance for `task`.
    Task,
}

impl ToolPrompt {
    /// Writes this tool's guidance into `output`.
    pub(crate) fn render(self, output: &mut String, facts: ToolPromptFacts) {
        tool_sections::render_tool(self, output, facts);
    }
}

/// Tracks which built-in tools are present.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolPromptFacts {
    has_allowed_path_tool: bool,
    has_bash: bool,
    has_read: bool,
    read_line_numbers: bool,
    has_write: bool,
    has_edit: bool,
    has_glob: bool,
    has_grep: bool,
}

impl ToolPromptFacts {
    /// Builds the tool facts from the tracked prompts.
    pub(crate) fn from_prompts(prompts: impl IntoIterator<Item = ToolPrompt>) -> Self {
        let mut facts = Self::default();
        for prompt in prompts {
            facts.record(prompt);
        }
        facts
    }

    /// Returns whether the shared `Common Rules` section has any content.
    pub(crate) fn has_common_rules(self) -> bool {
        self.has_allowed_path_tool
            || (self.has_bash
                && (self.has_glob
                    || self.has_grep
                    || self.has_read
                    || self.has_edit
                    || self.has_write))
            || ((self.has_glob as u8 + self.has_grep as u8 + self.has_read as u8) >= 2)
            || (self.has_edit && self.has_write)
            || (self.has_read && (self.has_edit || self.has_write))
    }

    /// Writes the shared `Common Rules` lines into `output`.
    pub(crate) fn write_common_rules(self, output: &mut String) {
        common_rules::write_common_rules(self, output);
    }

    fn record(&mut self, prompt: ToolPrompt) {
        match prompt {
            ToolPrompt::Static(_) => {}
            ToolPrompt::Bash { .. } => self.has_bash = true,
            ToolPrompt::Read {
                path_mode,
                line_numbers,
            } => {
                self.has_read = true;
                self.read_line_numbers |= line_numbers;
                self.note_path_mode(path_mode);
            }
            ToolPrompt::Write { path_mode } => {
                self.has_write = true;
                self.note_path_mode(path_mode);
            }
            ToolPrompt::Edit { path_mode } => {
                self.has_edit = true;
                self.note_path_mode(path_mode);
            }
            ToolPrompt::Glob { path_mode } => {
                self.has_glob = true;
                self.note_path_mode(path_mode);
            }
            ToolPrompt::Grep { path_mode, .. } => {
                self.has_grep = true;
                self.note_path_mode(path_mode);
            }
            ToolPrompt::WebFetch
            | ToolPrompt::TodoRead
            | ToolPrompt::TodoWrite
            | ToolPrompt::Task => {}
        }
    }

    fn note_path_mode(&mut self, path_mode: PathMode) {
        self.has_allowed_path_tool |= matches!(path_mode, PathMode::Allowed);
    }
}

pub(super) fn push_line(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
}

pub(super) fn push_block(output: &mut String, block: &str) {
    output.push_str(block);
}

pub(super) fn write_tool_list(output: &mut String, tools: &[&str]) {
    match tools {
        [] => {}
        [one] => write_tool_name(output, one),
        [left, right] => {
            write_tool_name(output, left);
            output.push_str(" and ");
            write_tool_name(output, right);
        }
        _ => {
            for (index, tool) in tools.iter().enumerate() {
                if index > 0 {
                    if index + 1 == tools.len() {
                        output.push_str(", and ");
                    } else {
                        output.push_str(", ");
                    }
                }
                write_tool_name(output, tool);
            }
        }
    }
}

fn write_tool_name(output: &mut String, tool: &str) {
    let _ = write!(output, "`{tool}`");
}
