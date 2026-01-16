//! Schema building utilities for tool parameter definitions.
//!
//! Provides composable helper functions and complete parameter schemas
//! using serdesAI's [`SchemaBuilder`]. This module is internal to the crate.

use serde_json::Value;
use serdes_ai::tools::SchemaBuilder;

// ============================================================================
// Composable Schema Helpers
// ============================================================================

/// Add required command property with minimum length constraint.
#[inline]
pub fn add_command(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string_constrained(
        "command",
        "The shell command to execute",
        true,
        Some(1),
        None,
        None,
    )
}

/// Add optional workdir property.
#[inline]
pub fn add_workdir(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string(
        "workdir",
        "Working directory for command execution (must be absolute path)",
        false,
    )
}

/// Add optional timeout_ms property with constraints.
#[inline]
pub fn add_timeout(builder: SchemaBuilder) -> SchemaBuilder {
    builder.integer_constrained(
        "timeout_ms",
        "Timeout in milliseconds. Defaults to 120000 (2 minutes).",
        false,
        Some(1),
        Some(600_000),
    )
}

/// Add required todos array property.
#[inline]
pub fn add_todos(builder: SchemaBuilder) -> SchemaBuilder {
    builder.raw(
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
}

/// Add required url property.
#[inline]
pub fn add_url(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string("url", "The URL to fetch", true)
}

/// Add required description property for task.
#[inline]
pub fn add_description(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string("description", "Short 3-5 word task description", true)
}

/// Add required prompt property for task.
#[inline]
pub fn add_prompt(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string("prompt", "Detailed instructions for the sub-agent", true)
}

/// Add required subagent_type property.
#[inline]
pub fn add_subagent_type(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string(
        "subagent_type",
        "Type of agent to use (e.g., \"general\", \"coder\")",
        true,
    )
}

/// Add optional session_id property.
#[inline]
pub fn add_session_id(builder: SchemaBuilder) -> SchemaBuilder {
    builder.string("session_id", "Existing session to continue", false)
}

// ============================================================================
// Complete Tool Schemas
// ============================================================================

/// Build a complete schema for the bash tool.
pub fn bash_schema() -> Result<Value, serde_json::Error> {
    add_timeout(add_workdir(add_command(SchemaBuilder::new()))).build()
}

/// Build a complete schema for the todo write tool.
pub fn todo_write_schema() -> Result<Value, serde_json::Error> {
    add_todos(SchemaBuilder::new()).build()
}

/// Build a complete schema for the todo read tool (empty object).
pub fn todo_read_schema() -> Result<Value, serde_json::Error> {
    SchemaBuilder::new().build()
}

/// Build a complete schema for the webfetch tool.
pub fn webfetch_schema() -> Result<Value, serde_json::Error> {
    add_timeout(add_url(SchemaBuilder::new())).build()
}

/// Build a complete schema for the task tool.
pub fn task_schema() -> Result<Value, serde_json::Error> {
    add_session_id(add_subagent_type(add_prompt(add_description(
        SchemaBuilder::new(),
    ))))
    .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_schema_has_command_constraints() {
        let schema = bash_schema().unwrap();
        let props = schema["properties"].as_object().unwrap();
        let command = props.get("command").unwrap();
        assert_eq!(command["minLength"], 1);
    }

    #[test]
    fn todo_write_schema_has_todos_required() {
        let schema = todo_write_schema().unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "todos"));
    }

    #[test]
    fn todo_read_schema_is_empty() {
        let schema = todo_read_schema().unwrap();
        let required = schema.get("required").and_then(|v| v.as_array());
        assert!(required.is_none() || required.unwrap().is_empty());
    }

    #[test]
    fn webfetch_schema_has_url_required() {
        let schema = webfetch_schema().unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "url"));
    }

    #[test]
    fn task_schema_has_required_fields() {
        let schema = task_schema().unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "description"));
        assert!(required.iter().any(|v| v == "prompt"));
        assert!(required.iter().any(|v| v == "subagent_type"));
        assert!(!required.iter().any(|v| v == "session_id"));
    }
}
