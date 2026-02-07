//! Task tool for invoking subagents (serdesAI adapter).
//!
//! Thin tool that validates access and dispatches to prebuilt agents in a registry.

use crate::convert::to_serdes_result;
use crate::registry::{AgentRegistry, RegistryAgent};
use async_trait::async_trait;
use llm_coding_tools_agents::{Ruleset, TaskInput};
use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::tool_names;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::sync::Arc;

/// Arguments for the Task tool (internal deserialization).
#[derive(Debug, Clone, Deserialize)]
struct TaskArgs {
    description: String,
    prompt: String,
    subagent_type: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
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

/// Task tool for serdesAI framework.
///
/// Validates access, builds the request message, and dispatches to stored agents.
pub struct TaskTool<A, Deps>
where
    A: RegistryAgent<Deps>,
{
    registry: Arc<AgentRegistry<A>>,
    caller_rules: Ruleset,
    deps: Arc<Deps>,
}

impl<A, Deps> TaskTool<A, Deps>
where
    A: RegistryAgent<Deps>,
{
    /// Creates a new Task tool with the given registry, caller permissions, and deps.
    ///
    /// Parameters:
    /// - `registry`: serdesAI agent registry.
    /// - `caller_rules`: permission rules for the calling agent.
    /// - `deps`: dependencies passed to registry agents.
    ///
    /// Returns: a new [`TaskTool`].
    pub fn new(registry: Arc<AgentRegistry<A>>, caller_rules: Ruleset, deps: Arc<Deps>) -> Self {
        Self {
            registry,
            caller_rules,
            deps,
        }
    }
}

#[async_trait]
impl<A, Deps, RuntimeDeps> Tool<RuntimeDeps> for TaskTool<A, Deps>
where
    A: RegistryAgent<Deps> + 'static,
    Deps: Send + Sync + 'static,
    RuntimeDeps: Send + Sync,
{
    fn definition(&self) -> ToolDefinition {
        // Build the Task tool description from invocable + permitted agents
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
            if !entry.is_invocable() {
                continue;
            }
            if !self.caller_rules.is_allowed("task", name) {
                continue;
            }
            lines.push(format!("- {}: {}", name, entry.tool_names.join(", ")));
        }

        let description = if lines.is_empty() {
            "Task tool is not available - no accessible agents.".to_string()
        } else {
            const TEMPLATE: &str = r#"Launch a new agent to handle complex, multistep tasks autonomously.

Available agent types and the tools they have access to:
{agents}

When using the Task tool, you must specify a subagent_type parameter to select which agent type to use."#;
            TEMPLATE.replace("{agents}", &lines.join("\n"))
        };

        ToolDefinition::new(tool_names::TASK, description).with_parameters(
            SchemaBuilder::new()
                .string_constrained(
                    "description",
                    "A short (3-5 words) description of the task",
                    true,
                    Some(1),
                    Some(100),
                    None,
                )
                .string_constrained(
                    "prompt",
                    "The task for the agent to perform",
                    true,
                    Some(1),
                    None,
                    None,
                )
                .string_constrained(
                    "subagent_type",
                    "The type of specialized agent to use for this task",
                    true,
                    Some(1),
                    None,
                    None,
                )
                .string("session_id", "Existing Task session to continue", false)
                .string("command", "The command that triggered this task", false)
                .build()
                .expect("schema serialization should never fail"),
        )
    }

    async fn call(&self, _ctx: &RunContext<RuntimeDeps>, args: serde_json::Value) -> ToolResult {
        let args: TaskArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::TASK, None, e.to_string()))?;

        let input: TaskInput = args.into();
        let entry = match self.registry.get(&input.subagent_type) {
            Some(entry) => entry,
            None => {
                return Err(ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Unknown agent type: {}", input.subagent_type),
                ));
            }
        };

        if !entry.is_invocable() {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!(
                    "Agent '{}' is not available for task invocation",
                    input.subagent_type
                ),
            ));
        }

        if !self.caller_rules.is_allowed("task", &input.subagent_type) {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!(
                    "Access denied: cannot invoke agent '{}'",
                    input.subagent_type
                ),
            ));
        }

        // Build the required task context user message
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
        message.push_str("</task_prompt>");

        let result = entry
            .agent
            .prompt(message, Arc::clone(&self.deps))
            .await
            .map_err(|err| {
                ToolError::execution_failed(format!("Task execution failed: {}", err))
            })?;

        to_serdes_result(
            tool_names::TASK,
            Ok(llm_coding_tools_core::ToolOutput::new(result)),
        )
    }
}

impl<A: RegistryAgent<Deps>, Deps> ToolContext for TaskTool<A, Deps> {
    const NAME: &'static str = tool_names::TASK;

    fn context(&self) -> &'static str {
        "Use the Task tool to delegate complex, multi-step tasks to specialized subagents."
    }
}

#[cfg(test)]
mod tests;
