//! Task execution types and mock executor.

use crate::error::ToolResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

/// Input arguments for task execution.
#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// Result from task execution.
#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    /// The task description.
    pub description: String,
    /// The agent type used.
    pub subagent_type: String,
    /// Session ID (new or continued).
    pub session_id: String,
    /// Result message from the agent.
    pub result: String,
}

impl TaskResult {
    /// Formats the result for display.
    pub fn format(&self) -> String {
        format!(
            "Task: {}\nAgent: {}\nSession: {}\nStatus: completed\n\nResult: {}",
            self.description, self.subagent_type, self.session_id, self.result
        )
    }
}

/// Trait for executing tasks.
///
/// Implement this to provide custom execution logic (e.g., real LLM agent).
#[async_trait]
pub trait TaskExecutor: Send + Sync {
    /// Execute a task with the given arguments.
    async fn execute(&self, args: &TaskArgs) -> ToolResult<TaskResult>;
}

/// Mock task executor for testing.
///
/// Returns predefined responses without LLM calls.
#[derive(Debug, Default)]
pub struct MockTaskExecutor {
    responses: RwLock<HashMap<String, String>>,
    session_counter: AtomicU64,
}

impl MockTaskExecutor {
    /// Creates a new mock executor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a custom response for a specific description.
    pub fn set_response(&self, description: impl Into<String>, response: impl Into<String>) {
        self.responses
            .write()
            .expect("lock poisoned")
            .insert(description.into(), response.into());
    }

    fn next_session_id(&self) -> String {
        let id = self.session_counter.fetch_add(1, Ordering::Relaxed);
        format!("mock-session-{id}")
    }
}

#[async_trait]
impl TaskExecutor for MockTaskExecutor {
    async fn execute(&self, args: &TaskArgs) -> ToolResult<TaskResult> {
        let session_id = args
            .session_id
            .clone()
            .unwrap_or_else(|| self.next_session_id());

        let result = self
            .responses
            .read()
            .expect("lock poisoned")
            .get(&args.description)
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    "Task '{}' completed successfully by {} agent.",
                    args.description, args.subagent_type
                )
            });

        Ok(TaskResult {
            description: args.description.clone(),
            subagent_type: args.subagent_type.clone(),
            session_id,
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_executor_returns_default_response() {
        let executor = MockTaskExecutor::new();
        let args = TaskArgs {
            description: "test task".into(),
            prompt: "do something".into(),
            subagent_type: "general".into(),
            session_id: None,
        };

        let result = executor.execute(&args).await.unwrap();

        assert_eq!(result.description, "test task");
        assert!(result.session_id.starts_with("mock-session-"));
        assert!(result.result.contains("test task"));
    }

    #[tokio::test]
    async fn mock_executor_uses_custom_response() {
        let executor = MockTaskExecutor::new();
        executor.set_response("custom task", "Custom result!");

        let args = TaskArgs {
            description: "custom task".into(),
            prompt: "details".into(),
            subagent_type: "coder".into(),
            session_id: None,
        };

        let result = executor.execute(&args).await.unwrap();
        assert_eq!(result.result, "Custom result!");
    }

    #[tokio::test]
    async fn mock_executor_continues_session() {
        let executor = MockTaskExecutor::new();
        let args = TaskArgs {
            description: "task".into(),
            prompt: "prompt".into(),
            subagent_type: "general".into(),
            session_id: Some("existing-session".into()),
        };

        let result = executor.execute(&args).await.unwrap();
        assert_eq!(result.session_id, "existing-session");
    }

    #[tokio::test]
    async fn session_ids_increment() {
        let executor = MockTaskExecutor::new();
        let args = TaskArgs {
            description: "task".into(),
            prompt: "prompt".into(),
            subagent_type: "general".into(),
            session_id: None,
        };

        let r1 = executor.execute(&args).await.unwrap();
        let r2 = executor.execute(&args).await.unwrap();

        assert_eq!(r1.session_id, "mock-session-0");
        assert_eq!(r2.session_id, "mock-session-1");
    }

    #[test]
    fn task_result_formats_correctly() {
        let result = TaskResult {
            description: "my task".into(),
            subagent_type: "coder".into(),
            session_id: "sess-1".into(),
            result: "Done!".into(),
        };

        let formatted = result.format();
        assert!(formatted.contains("Task: my task"));
        assert!(formatted.contains("Agent: coder"));
        assert!(formatted.contains("Session: sess-1"));
        assert!(formatted.contains("Result: Done!"));
    }
}
