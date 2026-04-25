//! Todo list management tools.
//!
//! Provides tools for reading and writing todo items.
//!
//! # Public API
//!
//! - [`TodoReadTool`] - read the current todo list
//! - [`TodoWriteTool`] - write/replace the todo list
//! - [`create_todo_tools`] - create a linked read/write pair with shared state
//! - [`Todo`], [`TodoPriority`], [`TodoStatus`], [`TodoState`] - core types

use async_trait::async_trait;
use reloaded_code_core::ToolOutput;
use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::tool_metadata::{
    todo_read as todo_read_meta, todo_write as todo_write_meta,
};
use reloaded_code_core::tools::{TodoReadRequest, TodoWriteRequest, read_todos, write_todos};
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolResult, ToolReturn};

use crate::convert::{core_error_to_serdes, to_serdes_result};

// Re-export core types
pub use reloaded_code_core::{Todo, TodoPriority, TodoState, TodoStatus};

/// Tool for writing/replacing the todo list.
#[derive(Debug, Clone)]
pub struct TodoWriteTool {
    definition: ToolDefinition,
    state: TodoState,
}

impl TodoWriteTool {
    /// Creates a new todo write tool with the given state.
    pub fn new(state: TodoState) -> Self {
        Self {
            definition: build_todo_write_definition(),
            state,
        }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for TodoWriteTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args = TodoWriteRequest::parse(args)
            .map_err(|e| core_error_to_serdes(todo_write_meta::NAME, e))?;
        let result = write_todos(&self.state, args);
        to_serdes_result(todo_write_meta::NAME, result.map(ToolOutput::new))
    }
}

impl ToolContext for TodoWriteTool {
    const NAME: &'static str = todo_write_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::TodoWrite
    }
}

/// Tool for reading the current todo list.
#[derive(Debug, Clone)]
pub struct TodoReadTool {
    definition: ToolDefinition,
    state: TodoState,
}

impl TodoReadTool {
    /// Creates a new todo read tool with the given state.
    pub fn new(state: TodoState) -> Self {
        Self {
            definition: build_todo_read_definition(),
            state,
        }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for TodoReadTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args = TodoReadRequest::parse(args)
            .map_err(|e| core_error_to_serdes(todo_read_meta::NAME, e))?;
        let output = read_todos(&self.state, args);
        Ok(ToolReturn::text(output))
    }
}

impl ToolContext for TodoReadTool {
    const NAME: &'static str = todo_read_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::TodoRead
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

fn build_todo_write_definition() -> ToolDefinition {
    ToolDefinition {
        name: todo_write_meta::NAME.to_owned(),
        description: todo_write_meta::DESCRIPTION.to_owned(),
        parameters_json_schema: SchemaBuilder::new()
            .raw(
                todo_write_meta::param::TODOS.name,
                serde_json::json!({
                    "type": "array",
                    "description": todo_write_meta::param::TODOS.description,
                    "items": {
                        "type": "object",
                        "required": [
                            todo_write_meta::param::ID.name,
                            todo_write_meta::param::CONTENT.name,
                            todo_write_meta::param::STATUS.name,
                            todo_write_meta::param::PRIORITY.name
                        ],
                        "properties": {
                            "id": { "type": "string", "description": todo_write_meta::param::ID.description },
                            "content": { "type": "string", "description": todo_write_meta::param::CONTENT.description },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"],
                                "description": todo_write_meta::param::STATUS.description
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["high", "medium", "low"],
                                "description": todo_write_meta::param::PRIORITY.description
                            }
                        }
                    }
                }),
                todo_write_meta::param::TODOS.required,
            )
            .build()
            .expect("schema serialization should never fail"),
        strict: None,
        outer_typed_dict_key: None,
    }
}

fn build_todo_read_definition() -> ToolDefinition {
    ToolDefinition {
        name: todo_read_meta::NAME.to_owned(),
        description: todo_read_meta::DESCRIPTION.to_owned(),
        parameters_json_schema: SchemaBuilder::new()
            .build()
            .expect("schema serialization should never fail"),
        strict: None,
        outer_typed_dict_key: None,
    }
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
