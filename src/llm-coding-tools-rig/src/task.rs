//! Task tool for launching autonomous sub-agents.
//!
//! Provides [`TaskTool`] for spawning sub-agents to handle complex tasks.

use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::{ToolContext, ToolError, ToolOutput};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::sync::Arc;

// Re-export core types
pub use llm_coding_tools_core::{
    MockTaskExecutor, TaskArgs as CoreTaskArgs, TaskExecutor, TaskResult,
};

/// Arguments for the task tool (with JsonSchema for rig).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TaskArgs {
    /// Short 3-5 word task description.
    pub description: String,
    /// Detailed instructions for the sub-agent.
    pub prompt: String,
    /// Type of agent to use (e.g., "general", "coder").
    pub subagent_type: String,
    /// Existing session to continue.
    #[serde(default)]
    pub session_id: Option<String>,
}

impl From<TaskArgs> for CoreTaskArgs {
    fn from(args: TaskArgs) -> Self {
        CoreTaskArgs {
            description: args.description,
            prompt: args.prompt,
            subagent_type: args.subagent_type,
            session_id: args.session_id,
        }
    }
}

/// Tool for delegating tasks to sub-agents.
///
/// Generic over the executor implementation.
#[derive(Debug, Clone)]
pub struct TaskTool<E: TaskExecutor> {
    executor: Arc<E>,
}

impl<E: TaskExecutor> TaskTool<E> {
    /// Creates a new task tool with the given executor.
    pub fn new(executor: Arc<E>) -> Self {
        Self { executor }
    }
}

impl TaskTool<MockTaskExecutor> {
    /// Creates a task tool with mock executor for testing.
    pub fn with_mock() -> (Self, Arc<MockTaskExecutor>) {
        let executor = Arc::new(MockTaskExecutor::new());
        (Self::new(executor.clone()), executor)
    }
}

impl<E: TaskExecutor + 'static> Tool for TaskTool<E> {
    const NAME: &'static str = tool_names::TASK;

    type Error = ToolError;
    type Args = TaskArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Delegate a task to a specialized sub-agent.".to_string(),
            parameters: serde_json::to_value(schema_for!(TaskArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let core_args = CoreTaskArgs::from(args);
        let result = self.executor.execute(&core_args).await?;
        Ok(ToolOutput::new(result.format()))
    }
}

impl<E: TaskExecutor> ToolContext for TaskTool<E> {
    const NAME: &'static str = tool_names::TASK;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::TASK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_executor_works() {
        let (tool, _executor) = TaskTool::with_mock();
        let args = TaskArgs {
            description: "test task".to_string(),
            prompt: "do something".to_string(),
            subagent_type: "general".to_string(),
            session_id: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.content.contains("test task"));
        assert!(result.content.contains("completed"));
    }

    #[tokio::test]
    async fn custom_mock_response() {
        let (tool, executor) = TaskTool::with_mock();
        executor.set_response("custom", "Custom result!");

        let args = TaskArgs {
            description: "custom".to_string(),
            prompt: "details".to_string(),
            subagent_type: "coder".to_string(),
            session_id: None,
        };
        let result = tool.call(args).await.unwrap();
        assert!(result.content.contains("Custom result!"));
    }
}
