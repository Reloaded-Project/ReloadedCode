//! Todo list management tools.
//!
//! Provides tools for reading and writing todo items.

use llm_coding_tools_core::operations::{read_todos, write_todos};
use llm_coding_tools_core::{ToolContext, ToolError, ToolOutput};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

// Re-export core types
pub use llm_coding_tools_core::{Todo, TodoPriority, TodoState, TodoStatus};

/// Arguments for writing todos.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TodoWriteArgs {
    /// The complete list of todos to set.
    pub todos: Vec<Todo>,
}

/// Arguments for reading todos (empty).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TodoReadArgs {}

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

impl Tool for TodoWriteTool {
    const NAME: &'static str = "TodoWrite";

    type Error = ToolError;
    type Args = TodoWriteArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Replace the todo list with new items.".to_string(),
            parameters: serde_json::to_value(schema_for!(TodoWriteArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let message = write_todos(&self.state, args.todos)?;
        Ok(ToolOutput::new(message))
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

impl Tool for TodoReadTool {
    const NAME: &'static str = "TodoRead";

    type Error = ToolError;
    type Args = TodoReadArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Read the current todo list.".to_string(),
            parameters: serde_json::to_value(schema_for!(TodoReadArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let content = read_todos(&self.state);
        Ok(ToolOutput::new(content))
    }
}

impl ToolContext for TodoWriteTool {
    const NAME: &'static str = "TodoWrite";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::TODO_WRITE
    }
}

impl ToolContext for TodoReadTool {
    const NAME: &'static str = "TodoRead";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::TODO_READ
    }
}

/// Helper for creating paired todo tools with shared state.
pub struct TodoTools {
    /// Tool for writing todos.
    pub write: TodoWriteTool,
    /// Tool for reading todos.
    pub read: TodoReadTool,
}

impl TodoTools {
    /// Creates new todo tools with shared state.
    pub fn new() -> Self {
        let state = TodoState::new();
        Self {
            write: TodoWriteTool::new(state.clone()),
            read: TodoReadTool::new(state),
        }
    }

    /// Creates todo tools with existing state.
    pub fn with_state(state: TodoState) -> Self {
        Self {
            write: TodoWriteTool::new(state.clone()),
            read: TodoReadTool::new(state),
        }
    }
}

impl Default for TodoTools {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_todo(id: &str, status: TodoStatus) -> Todo {
        Todo {
            id: id.to_string(),
            content: format!("Task {id}"),
            status,
            priority: TodoPriority::Medium,
        }
    }

    #[tokio::test]
    async fn write_and_read_todos() {
        let tools = TodoTools::new();

        let write_args = TodoWriteArgs {
            todos: vec![
                make_todo("1", TodoStatus::Pending),
                make_todo("2", TodoStatus::Completed),
            ],
        };
        let write_result = tools.write.call(write_args).await.unwrap();
        assert!(write_result.content.contains("2 task(s)"));

        let read_result = tools.read.call(TodoReadArgs {}).await.unwrap();
        assert!(read_result.content.contains("Task 1"));
        assert!(read_result.content.contains("Task 2"));
    }

    #[tokio::test]
    async fn shared_state_works() {
        let state = TodoState::new();
        let write_tool = TodoWriteTool::new(state.clone());
        let read_tool = TodoReadTool::new(state);

        let write_args = TodoWriteArgs {
            todos: vec![make_todo("shared", TodoStatus::InProgress)],
        };
        write_tool.call(write_args).await.unwrap();

        let read_result = read_tool.call(TodoReadArgs {}).await.unwrap();
        assert!(read_result.content.contains("shared"));
    }

    #[tokio::test]
    async fn empty_list_returns_no_tasks() {
        let tools = TodoTools::new();
        let result = tools.read.call(TodoReadArgs {}).await.unwrap();
        assert_eq!(result.content, "No tasks.");
    }
}
