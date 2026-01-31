//! Task tool for invoking subagents (rig adapter).
//!
//! Uses rig-native registry for direct agent lookup and invocation.

use llm_coding_tools_agents::{Ruleset, TaskInput};
use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::{ToolError, ToolOutput};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

use crate::registry::{AgentRegistry, RegistryAgent};

/// Arguments for the Task tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TaskArgs {
    /// A short (3-5 words) description of the task.
    pub description: String,
    /// The task for the agent to perform.
    pub prompt: String,
    /// The type of specialized agent to use for this task.
    pub subagent_type: String,
    /// Existing Task session to continue.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The command that triggered this task.
    #[serde(default)]
    pub command: Option<String>,
}

impl From<TaskArgs> for TaskInput {
    fn from(args: TaskArgs) -> Self {
        Self {
            description: args.description,
            prompt: args.prompt,
            subagent_type: args.subagent_type,
            session_id: args.session_id,
            command: args.command,
        }
    }
}

/// Task tool for rig framework.
///
/// Validates access, builds the request message, and dispatches to the stored agent.
pub struct TaskTool<A: RegistryAgent> {
    registry: Arc<AgentRegistry<A>>,
    caller_rules: Ruleset,
}

impl<A: RegistryAgent> TaskTool<A> {
    /// Creates a new Task tool with the given registry and caller permissions.
    ///
    /// Parameters:
    /// - `registry`: rig-native agent registry
    /// - `caller_rules`: permission rules for the calling agent
    ///
    /// Returns: a new [`TaskTool`].
    pub fn new(registry: Arc<AgentRegistry<A>>, caller_rules: Ruleset) -> Self {
        Self {
            registry,
            caller_rules,
        }
    }

    /// Builds the Task tool description, omitting hidden agents.
    ///
    /// Returns: description text for ToolDefinition.
    fn build_description(&self) -> String {
        let mut names: Vec<_> = self
            .registry
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        names.sort_unstable();

        let mut lines = Vec::with_capacity(names.len());
        for name in names {
            let entry = match self.registry.get(name) {
                Some(entry) => entry,
                None => continue,
            };
            if entry.config.hidden {
                continue;
            }
            if !entry.is_invocable() {
                continue;
            }
            if !self.caller_rules.is_allowed("task", name) {
                continue;
            }
            lines.push(format!("- {}: {}", name, entry.tool_names.join(", ")));
        }

        if lines.is_empty() {
            return "Task tool is not available - no accessible agents.".to_string();
        }

        const TEMPLATE: &str =
            "Launch a new agent to handle complex, multistep tasks autonomously.\n\nAvailable agent types and the tools they have access to:\n{agents}\n\nWhen using the Task tool, you must specify a subagent_type parameter to select which agent type to use.";
        TEMPLATE.replace("{agents}", &lines.join("\n"))
    }

    /// Builds the required task context user message.
    ///
    /// Parameters:
    /// - `input`: task input from tool args
    ///
    /// Returns: formatted user message string.
    fn build_task_message(input: &TaskInput) -> String {
        let mut message = String::with_capacity(input.prompt.len() + 128);
        message.push_str("<task_context>\n");
        message.push_str("description: ");
        message.push_str(&input.description);
        message.push('\n');
        message.push_str("command: ");
        if let Some(command) = &input.command {
            message.push_str(command);
        }
        message.push('\n');
        message.push_str("session_id: ");
        if let Some(session_id) = &input.session_id {
            message.push_str(session_id);
        }
        message.push('\n');
        message.push_str("</task_context>\n\n<task_prompt>\n");
        message.push_str(&input.prompt);
        message.push_str("\n</task_prompt>");
        message
    }
}

impl<A: RegistryAgent + 'static> Tool for TaskTool<A> {
    const NAME: &'static str = tool_names::TASK;

    type Error = ToolError;
    type Args = TaskArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: self.build_description(),
            parameters: serde_json::to_value(schemars::schema_for!(TaskArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input: TaskInput = args.into();

        let entry = match self.registry.get(&input.subagent_type) {
            Some(entry) => entry,
            None => {
                return Err(ToolError::Validation(format!(
                    "Unknown agent type: {}",
                    input.subagent_type
                )))
            }
        };

        if !entry.is_invocable() {
            return Err(ToolError::Validation(format!(
                "Agent '{}' is not available for task invocation",
                input.subagent_type
            )));
        }

        if !self.caller_rules.is_allowed("task", &input.subagent_type) {
            return Err(ToolError::Validation(format!(
                "Access denied: cannot invoke agent '{}'",
                input.subagent_type
            )));
        }

        let message = Self::build_task_message(&input);
        let result = entry.agent.prompt(message).await.map_err(|err| {
            ToolError::Execution(format!("Task execution failed: {}", err.message))
        })?;

        Ok(ToolOutput::new(result))
    }
}

#[cfg(test)]
mod tests;
