//! Todo list management tools.
//!
//! Provides tools for reading and writing todo items.

use crate::convert::to_serdes_result;
use async_trait::async_trait;
use llm_coding_tools_core::ToolOutput;
use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::operations::{read_todos, write_todos};
use llm_coding_tools_core::tool_names;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};

// Re-export core types
pub use llm_coding_tools_core::{Todo, TodoPriority, TodoState, TodoStatus};

/// Arguments for writing todos.
#[derive(Debug, Clone, Deserialize)]
struct TodoWriteArgs {
    /// The complete list of todos to set.
    todos: Vec<Todo>,
}

/// Arguments for reading todos.
///
/// Empty struct required for consistent JSON validation via [`serde_json::from_value`].
/// Ensures the input is a valid JSON object even when no parameters are needed.
#[derive(Debug, Clone, Deserialize)]
struct TodoReadArgs {}

/// Tool for writing/replacing the todo list.
#[derive(Debug, Clone)]
pub struct TodoWriteTool {
    state: TodoState,
}

impl TodoWriteTool {
    /// Creates a new todo write tool with the given state.
    pub fn new(state: TodoState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for TodoWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            tool_names::TODO_WRITE,
            "Replace the todo list with new items.",
        )
        .with_parameters(
            SchemaBuilder::new()
                .raw(
                    "todos",
                    serde_json::json!({
                        "type": "array",
                        "description": "The complete list of todos to set",
                        "items": {
                            "type": "object",
                            "required": ["id", "content", "status", "priority"],
                            "properties": {
                                "id": { "type": "string", "description": "Unique identifier" },
                                "content": { "type": "string", "description": "Task description" },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed", "cancelled"],
                                    "description": "Current status"
                                },
                                "priority": {
                                    "type": "string",
                                    "enum": ["high", "medium", "low"],
                                    "description": "Priority level"
                                }
                            }
                        }
                    }),
                    true,
                )
                .build()
                .expect("schema serialization should never fail"),
        )
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: TodoWriteArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::validation_error(tool_names::TODO_WRITE, None, e.to_string())
        })?;
        let result = write_todos(&self.state, args.todos);
        to_serdes_result(tool_names::TODO_WRITE, result.map(ToolOutput::new))
    }
}

impl ToolContext for TodoWriteTool {
    const NAME: &'static str = tool_names::TODO_WRITE;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::TODO_WRITE
    }
}

/// Tool for reading the current todo list.
#[derive(Debug, Clone)]
pub struct TodoReadTool {
    state: TodoState,
}

impl TodoReadTool {
    /// Creates a new todo read tool with the given state.
    pub fn new(state: TodoState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for TodoReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(tool_names::TODO_READ, "Read the current todo list.").with_parameters(
            SchemaBuilder::new()
                .build()
                .expect("schema serialization should never fail"),
        )
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        // Validate JSON is a proper object (empty struct validates this)
        let _args: TodoReadArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::TODO_READ, None, e.to_string()))?;
        let content = read_todos(&self.state);
        Ok(crate::convert::output_to_return(ToolOutput::new(content)))
    }
}

impl ToolContext for TodoReadTool {
    const NAME: &'static str = tool_names::TODO_READ;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::TODO_READ
    }
}

/// Creates a pair of todo tools with shared state.
///
/// Returns `(TodoReadTool, TodoWriteTool, TodoState)` for cases where
/// the caller needs access to the underlying state.
pub fn create_todo_tools() -> (TodoReadTool, TodoWriteTool, TodoState) {
    let state = TodoState::new();
    (
        TodoReadTool::new(state.clone()),
        TodoWriteTool::new(state.clone()),
        state,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_ctx() -> RunContext<()> {
        RunContext::minimal("test-model")
    }

    #[tokio::test]
    async fn write_and_read_todos() {
        let (read, write, _state) = create_todo_tools();

        let write_args = serde_json::json!({
            "todos": [
                { "id": "1", "content": "Task 1", "status": "pending", "priority": "medium" },
                { "id": "2", "content": "Task 2", "status": "completed", "priority": "high" }
            ]
        });
        let write_result = write.call(&mock_ctx(), write_args).await.unwrap();
        assert!(write_result.as_text().unwrap().contains("2 task(s)"));

        let read_result = read.call(&mock_ctx(), serde_json::json!({})).await.unwrap();
        let text = read_result.as_text().unwrap();
        assert!(text.contains("Task 1"));
        assert!(text.contains("Task 2"));
    }

    #[tokio::test]
    async fn shared_state_works() {
        let state = TodoState::new();
        let write_tool = TodoWriteTool::new(state.clone());
        let read_tool = TodoReadTool::new(state);

        let write_args = serde_json::json!({
            "todos": [{ "id": "shared", "content": "Shared task", "status": "in_progress", "priority": "low" }]
        });
        write_tool.call(&mock_ctx(), write_args).await.unwrap();

        let read_result = read_tool
            .call(&mock_ctx(), serde_json::json!({}))
            .await
            .unwrap();
        assert!(read_result.as_text().unwrap().contains("shared"));
    }

    #[tokio::test]
    async fn empty_list_returns_no_tasks() {
        let (read, _write, _state) = create_todo_tools();
        let result = read.call(&mock_ctx(), serde_json::json!({})).await.unwrap();
        assert_eq!(result.as_text().unwrap(), "No tasks.");
    }
}
