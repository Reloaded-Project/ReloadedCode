//! Task tool for invoking subagents (serdesAI adapter).
//!
//! Thin wrapper around [`TaskToolCore`] for serdesAI framework compatibility.
//!
//! **Note:** This adapter stores `deps: Arc<R::Deps>` in the struct, not retrieving
//! from `RunContext`. This is consistent with other serdesAI tools that ignore `_ctx`.

use crate::convert::to_serdes_result;
use async_trait::async_trait;
use llm_coding_tools_agents::{
    Ruleset, TaskError as AgentTaskError, TaskInput, TaskRunner, TaskToolCore,
};
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
/// Wraps [`TaskToolCore`] to provide subagent invocation capabilities.
/// **Stores deps in struct** - does NOT use `ctx.deps` from RunContext.
///
/// # Type Parameters
///
/// * `R` - The [`TaskRunner`] implementation
pub struct TaskTool<R: TaskRunner> {
    core: TaskToolCore<R>,
    deps: Arc<R::Deps>,
}

impl<R: TaskRunner> TaskTool<R> {
    /// Creates a new Task tool with the given runner, caller permissions, and deps.
    pub fn new(runner: Arc<R>, caller_rules: Ruleset, deps: Arc<R::Deps>) -> Self {
        Self {
            core: TaskToolCore::new(runner, caller_rules),
            deps,
        }
    }

    /// Returns the core task tool logic.
    #[inline]
    pub fn core(&self) -> &TaskToolCore<R> {
        &self.core
    }
}

impl<R: TaskRunner> Clone for TaskTool<R> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            deps: Arc::clone(&self.deps),
        }
    }
}

#[async_trait]
impl<R, Deps> Tool<Deps> for TaskTool<R>
where
    R: TaskRunner + 'static,
    Deps: Send + Sync,
{
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(tool_names::TASK, self.core.build_description()).with_parameters(
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

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: TaskArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::TASK, None, e.to_string()))?;

        let input: TaskInput = args.into();

        // Use self.deps, NOT ctx.deps (consistent with other serdesAI tools)
        let result = self
            .core
            .execute(input, &self.deps)
            .await
            .map_err(|e| match e {
                AgentTaskError::UnknownAgent(name) => ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Unknown agent type: {}", name),
                ),
                AgentTaskError::AccessDenied(name) => ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Access denied: cannot invoke agent '{}'", name),
                ),
                AgentTaskError::NotInvocable(name) => ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Agent '{}' is not available for task invocation", name),
                ),
                AgentTaskError::Execution(msg) => ToolError::execution_failed(msg),
                AgentTaskError::Configuration(msg) => {
                    ToolError::validation_error(tool_names::TASK, None, msg)
                }
            })?;

        to_serdes_result(
            tool_names::TASK,
            Ok(llm_coding_tools_core::ToolOutput::new(result.format())),
        )
    }
}

impl<R: TaskRunner> ToolContext for TaskTool<R> {
    const NAME: &'static str = tool_names::TASK;

    fn context(&self) -> &'static str {
        "Use the Task tool to delegate complex, multi-step tasks to specialized subagents."
    }
}

#[cfg(test)]
mod tests;
