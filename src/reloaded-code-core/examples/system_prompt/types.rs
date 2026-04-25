use reloaded_code_core::context::PathMode;
use serde_json::Value;

use super::sort_sizes_desc;

/// Configures the `read` tool for one example case.
#[derive(Debug, Clone, Copy)]
pub struct ReadConfig {
    pub path_mode: PathMode,
    pub line_numbers: bool,
}

/// Configures the `grep` tool for one example case.
#[derive(Debug, Clone, Copy)]
pub struct GrepConfig {
    pub path_mode: PathMode,
    pub line_numbers: bool,
}

/// Describes one subagent target for the `task` example definition.
#[derive(Debug, Clone, Copy)]
pub struct TaskTarget {
    pub name: &'static str,
    pub description: &'static str,
}

/// Describes one system prompt example scenario.
#[derive(Debug, Clone, Copy)]
pub struct PromptCase {
    pub system_prompt: &'static str,
    pub working_directory: Option<&'static str>,
    pub allowed_paths: &'static [&'static str],
    pub include_git_workflow: bool,
    pub include_github_cli: bool,
    pub read: Option<ReadConfig>,
    pub write: Option<PathMode>,
    pub edit: Option<PathMode>,
    pub bash: bool,
    pub glob: Option<PathMode>,
    pub grep: Option<GrepConfig>,
    pub webfetch: bool,
    pub todo_write: bool,
    pub todo_read: bool,
    pub task_targets: &'static [TaskTarget],
}

impl PromptCase {
    /// Returns the same case without supplemental git workflow sections.
    pub fn without_supplemental(mut self) -> Self {
        self.include_git_workflow = false;
        self.include_github_cli = false;
        self
    }
}

/// Holds the rendered prompt and serialized tool definitions for one case.
pub struct PromptArtifacts {
    pub system_prompt: String,
    pub tool_definitions: Vec<Value>,
    pub tool_definition_payload: String,
    pub guideline_sections: Vec<(String, usize)>,
}

impl PromptArtifacts {
    /// Returns the combined character count of prompt text and tool definitions.
    pub fn total_chars(&self) -> usize {
        self.system_prompt.len() + self.tool_definition_payload.len()
    }

    /// Returns serialized tool-definition sizes sorted from largest to smallest.
    pub fn definition_sizes(&self) -> Vec<(String, usize)> {
        let mut sizes: Vec<_> = self
            .tool_definitions
            .iter()
            .map(|tool| {
                let name = tool["name"].as_str().unwrap_or("unknown");
                let chars = serde_json::to_string(tool).unwrap().len();
                (name.to_string(), chars)
            })
            .collect();
        sort_sizes_desc(&mut sizes);
        sizes
    }
}
