//! Task tool for launching autonomous sub-agents.
//!
//! Provides [`TaskTool`] for spawning sub-agents to handle complex tasks.

use crate::convert::to_serdes_result;
use crate::schema::task_schema;
use async_trait::async_trait;
use llm_coding_tools_core::ToolOutput;
use llm_coding_tools_core::context::ToolContext;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, Tool, ToolDefinition, ToolError, ToolResult};
use std::sync::Arc;

/// Convenience re-exports from [`llm_coding_tools_core`] for users of this crate.
///
/// Re-exports:
/// - [`MockTaskExecutor`]: Mock implementation for testing.
/// - [`TaskExecutor`]: Trait for executing sub-agent tasks.
/// - `CoreTaskArgs` (aliased from `TaskArgs`): Arguments for task execution.
/// - `CoreTaskResult` (aliased from `TaskResult`): Result of task execution.
pub use llm_coding_tools_core::{
    MockTaskExecutor, TaskArgs as CoreTaskArgs, TaskExecutor, TaskResult as CoreTaskResult,
};

/// Arguments for the task tool.
#[derive(Debug, Clone, Deserialize)]
struct TaskArgs {
    /// Short 3-5 word task description.
    description: String,
    /// Detailed instructions for the sub-agent.
    prompt: String,
    /// Type of agent to use (e.g., "general", "coder").
    subagent_type: String,
    /// Existing session to continue.
    #[serde(default)]
    session_id: Option<String>,
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
/// Generic over the executor implementation. The executor must implement
/// [`TaskExecutor`] which requires `Send + Sync`.
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

// Note: TaskExecutor already requires Send + Sync in its trait definition.
// The 'static bound is needed for type erasure in async contexts.
#[async_trait]
impl<Deps: Send + Sync, E: TaskExecutor + Send + Sync + 'static> Tool<Deps> for TaskTool<E> {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("Task", "Delegate a task to a specialized sub-agent.")
            .with_parameters(task_schema().expect("schema serialization should never fail"))
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: TaskArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("Task", None, e.to_string()))?;
        let core_args = CoreTaskArgs::from(args);
        let result = self.executor.execute(&core_args).await;
        to_serdes_result("Task", result.map(|r| ToolOutput::new(r.format())))
    }
}

impl<E: TaskExecutor> ToolContext for TaskTool<E> {
    const NAME: &'static str = "Task";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::TASK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_ctx() -> RunContext<()> {
        RunContext::minimal("test-model")
    }

    #[tokio::test]
    async fn mock_executor_works() {
        let (tool, _executor) = TaskTool::with_mock();
        let args = serde_json::json!({
            "description": "test task",
            "prompt": "do something",
            "subagent_type": "general"
        });
        let result = tool.call(&mock_ctx(), args).await.unwrap();
        let text = result.as_text().unwrap();
        assert!(text.contains("test task"));
        assert!(text.contains("completed"));
    }

    #[tokio::test]
    async fn custom_mock_response() {
        let (tool, executor) = TaskTool::with_mock();
        executor.set_response("custom", "Custom result!");

        let args = serde_json::json!({
            "description": "custom",
            "prompt": "details",
            "subagent_type": "coder"
        });
        let result = tool.call(&mock_ctx(), args).await.unwrap();
        assert!(result.as_text().unwrap().contains("Custom result!"));
    }

    /// Test executor that returns errors for testing error propagation.
    #[derive(Debug)]
    struct ErrorExecutor;

    #[async_trait]
    impl TaskExecutor for ErrorExecutor {
        async fn execute(
            &self,
            _args: &CoreTaskArgs,
        ) -> Result<CoreTaskResult, llm_coding_tools_core::ToolError> {
            Err(llm_coding_tools_core::ToolError::Execution(
                "simulated executor failure".into(),
            ))
        }
    }

    #[tokio::test]
    async fn error_propagation_through_to_serdes_result() {
        let executor = Arc::new(ErrorExecutor);
        let tool = TaskTool::new(executor);

        let args = serde_json::json!({
            "description": "failing task",
            "prompt": "this will fail",
            "subagent_type": "general"
        });

        let result = tool.call(&mock_ctx(), args).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        // Execution errors should map to ExecutionFailed, not ValidationFailed
        assert!(!matches!(
            err,
            serdes_ai::tools::ToolError::ValidationFailed { .. }
        ));
        assert!(err.message().contains("execution error"));
        assert!(err.message().contains("simulated executor failure"));
    }
}
