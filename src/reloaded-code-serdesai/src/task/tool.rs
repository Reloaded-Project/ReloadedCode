//! Concrete [`Tool`] implementation that exposes the Task tool to the SerdesAI
//! runtime.
//!
//! [`TaskTool`] is constructed per-caller with a set of callable targets and a
//! shared [`TaskHandle`]. Each invocation deserialises a [`TaskInput`], delegates
//! to the handle, and returns the [`TaskOutput`] as JSON.
//!
//! [`Tool`]: serdes_ai::tools::Tool
//! [`TaskHandle`]: crate::task::TaskHandle
//! [`TaskInput`]: reloaded_code_core::TaskInput
//! [`TaskOutput`]: reloaded_code_core::TaskOutput

use crate::task::{TaskHandle, task_tool_definition};
use async_trait::async_trait;
use reloaded_code_agents::TaskTargetSummary;
use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::tool_metadata::task as task_meta;
use reloaded_code_core::{CredentialLookup, CredentialResolver, TaskInput};
use serdes_ai::tools::{RunContext, Tool, ToolDefinition, ToolError, ToolResult, ToolReturn};

/// One-shot Task tool wired into the SerdesAI runtime.
#[derive(Clone)]
pub(crate) struct TaskTool<C: CredentialLookup + Send + Sync + 'static = CredentialResolver> {
    caller_name: Box<str>,
    definition: ToolDefinition,
    handle: TaskHandle<C>,
}

impl<C> TaskTool<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    /// Creates a new Task tool for one caller and its callable targets.
    pub(crate) fn new(
        caller_name: impl Into<Box<str>>,
        targets: Vec<TaskTargetSummary>,
        handle: TaskHandle<C>,
    ) -> Self {
        Self {
            caller_name: caller_name.into(),
            definition: task_tool_definition(&targets),
            handle,
        }
    }
}

#[async_trait]
impl<Deps: Send + Sync, C> Tool<Deps> for TaskTool<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    /// Deserialises `args` as [`TaskInput`], delegates to [`TaskHandle::execute`],
    /// and returns the result as JSON.
    ///
    /// # Errors
    ///
    /// - Returns [`ToolError::ValidationFailed`] when `args` cannot be parsed as
    ///   a [`TaskInput`].
    /// - Propagates any error from [`TaskHandle::execute`] (validation or
    ///   execution failures).
    ///
    /// [`TaskHandle::execute`]: crate::task::TaskHandle::execute
    /// [`TaskInput`]: reloaded_code_core::TaskInput
    /// [`ToolError::ValidationFailed`]: serdes_ai::tools::ToolError
    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let input: TaskInput = serde_json::from_value(args)
            .map_err(|err| ToolError::validation_error(task_meta::NAME, None, err.to_string()))?;
        let output = self
            .handle
            .execute(self.caller_name.as_ref(), input)
            .await?;
        let payload =
            serde_json::to_value(output).expect("TaskOutput serialization should never fail");
        Ok(ToolReturn::json(payload))
    }
}

impl<C> ToolContext for TaskTool<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        task_meta::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Task
    }
}

#[cfg(test)]
mod tests {
    use super::{task_meta, *};

    fn summary(name: &str, description: &str) -> TaskTargetSummary {
        TaskTargetSummary {
            name: name.into(),
            description: description.into(),
        }
    }

    #[test]
    fn task_tool_definition_matches_target_set() {
        let targets = vec![
            summary("alpha", "Alpha agent"),
            summary("beta", "Beta agent"),
        ];

        let definition = task_tool_definition(&targets);
        assert_eq!(definition.name(), task_meta::NAME);
        assert!(!definition.description().is_empty());
        assert!(definition.description().contains("alpha"));
        assert!(definition.description().contains("beta"));
    }

    #[test]
    fn task_tool_name_matches_metadata() {
        let targets = vec![
            summary("alpha", "Alpha agent"),
            summary("beta", "Beta agent"),
        ];
        let definition = task_tool_definition(&targets);
        assert_eq!(definition.name(), task_meta::NAME);
    }
}
