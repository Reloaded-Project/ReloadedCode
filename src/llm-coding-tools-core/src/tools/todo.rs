//! Todo list management operation.

use crate::error::{ToolError, ToolResult};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use std::sync::Arc;

/// Task status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    /// Not yet started.
    Pending,
    /// Currently being worked on.
    InProgress,
    /// Successfully finished.
    Completed,
    /// Abandoned or no longer relevant.
    Cancelled,
}

impl TodoStatus {
    /// Returns the status indicator icon.
    #[inline]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[>]",
            Self::Completed => "[x]",
            Self::Cancelled => "[-]",
        }
    }
}

/// Task priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    /// Urgent, should be addressed first.
    High,
    /// Normal priority.
    Medium,
    /// Can be deferred.
    Low,
}

/// A single task item.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Todo {
    /// Unique identifier for the task.
    pub id: String,
    /// Task description.
    pub content: String,
    /// Current status.
    pub status: TodoStatus,
    /// Priority level.
    pub priority: TodoPriority,
}

/// Thread-safe shared state for todo list.
#[derive(Debug, Clone, Default)]
pub struct TodoState {
    todos: Arc<RwLock<Vec<Todo>>>,
}

impl TodoState {
    /// Creates a new empty todo state.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Writes/replaces the todo list with new items.
///
/// Validates that all todos have non-empty id and content.
pub fn write_todos(state: &TodoState, todos: Vec<Todo>) -> ToolResult<String> {
    for todo in &todos {
        if todo.id.trim().is_empty() {
            return Err(ToolError::Validation("todo id cannot be empty".into()));
        }
        if todo.content.trim().is_empty() {
            return Err(ToolError::Validation("todo content cannot be empty".into()));
        }
    }

    let count = todos.len();
    *state.todos.write() = todos;
    Ok(format!("Updated todo list with {count} task(s)."))
}

/// Reads and formats the current todo list.
pub fn read_todos(state: &TodoState) -> String {
    let todos = state.todos.read();

    if todos.is_empty() {
        return "No tasks.".to_string();
    }

    let mut output = format!("Tasks ({} total):\n", todos.len());
    for todo in todos.iter() {
        let _ = writeln!(
            output,
            "{} ({:?}) {}: {}",
            todo.status.icon(),
            todo.priority,
            todo.id,
            todo.content
        );
    }

    output.truncate(output.trim_end().len());
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn make_todo(id: &str, status: TodoStatus) -> Todo {
        Todo {
            id: id.to_string(),
            content: format!("Task {id}"),
            status,
            priority: TodoPriority::Medium,
        }
    }

    /// Verifies that write_todos rejects todos with empty id or content.
    #[rstest]
    #[case::empty_id("", "Task", "id")]
    #[case::empty_content("1", "  ", "content")]
    fn write_validates_required_fields(
        #[case] id: &str,
        #[case] content: &str,
        #[case] _field_name: &str,
    ) {
        let state = TodoState::new();
        let todo = Todo {
            id: id.to_string(),
            content: content.to_string(),
            status: TodoStatus::Pending,
            priority: TodoPriority::Low,
        };
        let result = write_todos(&state, vec![todo]);
        assert!(matches!(result, Err(ToolError::Validation(_))));
    }

    /// Verifies that each status variant returns the correct icon string.
    #[rstest]
    #[case::pending(TodoStatus::Pending, "[ ]")]
    #[case::in_progress(TodoStatus::InProgress, "[>]")]
    #[case::completed(TodoStatus::Completed, "[x]")]
    #[case::cancelled(TodoStatus::Cancelled, "[-]")]
    fn status_icons(#[case] status: TodoStatus, #[case] expected: &str) {
        assert_eq!(status.icon(), expected);
    }

    #[test]
    fn write_and_read_todos() {
        let state = TodoState::new();

        let todos = vec![
            make_todo("1", TodoStatus::Completed),
            make_todo("2", TodoStatus::InProgress),
            make_todo("3", TodoStatus::Pending),
        ];

        let result = write_todos(&state, todos).unwrap();
        assert!(result.contains("3 task(s)"));

        let output = read_todos(&state);
        assert!(output.contains("[x]"));
        assert!(output.contains("[>]"));
        assert!(output.contains("[ ]"));
    }

    #[test]
    fn read_empty_list() {
        let state = TodoState::new();
        let output = read_todos(&state);
        assert_eq!(output, "No tasks.");
    }

    #[test]
    fn write_replaces_existing() {
        let state = TodoState::new();

        write_todos(&state, vec![make_todo("a", TodoStatus::Pending)]).unwrap();
        write_todos(&state, vec![make_todo("b", TodoStatus::Completed)]).unwrap();

        let output = read_todos(&state);
        assert!(!output.contains("Task a"));
        assert!(output.contains("Task b"));
    }

    #[test]
    fn status_serde_roundtrip() {
        let json = serde_json::to_string(&TodoStatus::InProgress).unwrap();
        assert_eq!(json, "\"in_progress\"");
        let parsed: TodoStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TodoStatus::InProgress);
    }
}
