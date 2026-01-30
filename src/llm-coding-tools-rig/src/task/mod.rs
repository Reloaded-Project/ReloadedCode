//! Task tool for invoking subagents (rig adapter).
//!
//! Thin wrapper around [`TaskToolCore`] for rig framework compatibility.

use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::{ToolError, ToolOutput};
use llm_coding_tools_subagents::{
    Ruleset, TaskError as SubagentTaskError, TaskInput, TaskRunner, TaskToolCore,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

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
/// Wraps [`TaskToolCore`] to provide subagent invocation capabilities.
/// Stores deps in struct - does NOT require `Deps: Default`.
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

impl<R: TaskRunner + 'static> Tool for TaskTool<R> {
    const NAME: &'static str = tool_names::TASK;

    type Error = ToolError;
    type Args = TaskArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: self.core.build_description(),
            parameters: serde_json::to_value(schemars::schema_for!(TaskArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input: TaskInput = args.into();

        let result = self
            .core
            .execute(input, &self.deps)
            .await
            .map_err(|e| match e {
                SubagentTaskError::UnknownAgent(name) => {
                    ToolError::Validation(format!("Unknown agent type: {}", name))
                }
                SubagentTaskError::AccessDenied(name) => ToolError::Validation(format!(
                    "Access denied: cannot invoke subagent '{}'",
                    name
                )),
                SubagentTaskError::NotInvocable(name) => ToolError::Validation(format!(
                    "Subagent '{}' is not available for task invocation",
                    name
                )),
                SubagentTaskError::Execution(msg) => ToolError::Execution(msg),
                SubagentTaskError::Configuration(msg) => ToolError::Validation(msg),
            })?;

        Ok(ToolOutput::new(result.format()))
    }
}

#[cfg(test)]
mod tests;
