//! Tool-definition builders for system prompt preview examples.

use reloaded_code_core::context::PathMode;
use reloaded_code_core::tool_metadata;
use serde_json::{json, Map, Value};

use super::{PromptCase, TaskTarget};

/// Builds the tool definitions that match one example case.
pub(super) fn tool_definitions_for_case(case: PromptCase) -> Vec<Value> {
    let mut definitions = Vec::with_capacity(10);
    push_read_definition(&mut definitions, case);
    push_write_definition(&mut definitions, case);
    push_edit_definition(&mut definitions, case);
    push_bash_definition(&mut definitions, case);
    push_glob_definition(&mut definitions, case);
    push_grep_definition(&mut definitions, case);
    push_webfetch_definition(&mut definitions, case);
    push_todo_write_definition(&mut definitions, case);
    push_todo_read_definition(&mut definitions, case);
    push_task_definition(&mut definitions, case);
    definitions
}

fn push_read_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    let Some(read) = case.read else {
        return;
    };

    let file_path = match read.path_mode {
        PathMode::Absolute => tool_metadata::read::param::FILE_PATH_ABSOLUTE,
        PathMode::Allowed => tool_metadata::read::param::FILE_PATH_ALLOWED,
    };
    let description = match read.path_mode {
        PathMode::Absolute => tool_metadata::read::description::absolute(read.line_numbers),
        PathMode::Allowed => tool_metadata::read::description::allowed(read.line_numbers),
    };

    definitions.push(tool_definition(
        tool_metadata::read::NAME,
        description,
        object_schema(
            vec![
                (file_path.name, string_schema(file_path.description)),
                (
                    tool_metadata::read::param::OFFSET.name,
                    integer_schema(
                        tool_metadata::read::param::OFFSET.description,
                        Some(1),
                        None,
                    ),
                ),
                (
                    tool_metadata::read::param::LIMIT.name,
                    integer_schema(tool_metadata::read::param::LIMIT.description, Some(1), None),
                ),
            ],
            &[file_path.name],
        ),
    ));
}

fn push_write_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    let Some(write) = case.write else {
        return;
    };

    let file_path = match write {
        PathMode::Absolute => tool_metadata::write::param::FILE_PATH_ABSOLUTE,
        PathMode::Allowed => tool_metadata::write::param::FILE_PATH_ALLOWED,
    };
    let description = match write {
        PathMode::Absolute => tool_metadata::write::description::ABSOLUTE,
        PathMode::Allowed => tool_metadata::write::description::ALLOWED,
    };

    definitions.push(tool_definition(
        tool_metadata::write::NAME,
        description,
        object_schema(
            vec![
                (file_path.name, string_schema(file_path.description)),
                (
                    tool_metadata::write::param::CONTENT.name,
                    string_schema(tool_metadata::write::param::CONTENT.description),
                ),
            ],
            &[file_path.name, tool_metadata::write::param::CONTENT.name],
        ),
    ));
}

fn push_edit_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    let Some(edit) = case.edit else {
        return;
    };

    let file_path = match edit {
        PathMode::Absolute => tool_metadata::edit::param::FILE_PATH_ABSOLUTE,
        PathMode::Allowed => tool_metadata::edit::param::FILE_PATH_ALLOWED,
    };
    let description = match edit {
        PathMode::Absolute => tool_metadata::edit::description::ABSOLUTE,
        PathMode::Allowed => tool_metadata::edit::description::ALLOWED,
    };

    definitions.push(tool_definition(
        tool_metadata::edit::NAME,
        description,
        object_schema(
            vec![
                (file_path.name, string_schema(file_path.description)),
                (
                    tool_metadata::edit::param::OLD_STRING.name,
                    string_schema(tool_metadata::edit::param::OLD_STRING.description),
                ),
                (
                    tool_metadata::edit::param::NEW_STRING.name,
                    string_schema(tool_metadata::edit::param::NEW_STRING.description),
                ),
                (
                    tool_metadata::edit::param::REPLACE_ALL.name,
                    boolean_schema(tool_metadata::edit::param::REPLACE_ALL.description),
                ),
            ],
            &[
                file_path.name,
                tool_metadata::edit::param::OLD_STRING.name,
                tool_metadata::edit::param::NEW_STRING.name,
            ],
        ),
    ));
}

fn push_bash_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    if !case.bash {
        return;
    }

    definitions.push(tool_definition(
        tool_metadata::bash::NAME,
        tool_metadata::bash::DESCRIPTION,
        object_schema(
            vec![
                (
                    tool_metadata::bash::param::COMMAND.name,
                    string_schema_constrained(
                        tool_metadata::bash::param::COMMAND.description,
                        Some(1),
                        None,
                    ),
                ),
                (
                    tool_metadata::bash::param::WORKDIR.name,
                    string_schema(tool_metadata::bash::param::WORKDIR.description),
                ),
                (
                    tool_metadata::bash::param::TIMEOUT_MS.name,
                    integer_schema(
                        tool_metadata::bash::param::TIMEOUT_MS.description,
                        Some(1),
                        Some(tool_metadata::bash::MAX_TIMEOUT_MS as i64),
                    ),
                ),
            ],
            &[tool_metadata::bash::param::COMMAND.name],
        ),
    ));
}

fn push_glob_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    let Some(glob) = case.glob else {
        return;
    };

    let path = match glob {
        PathMode::Absolute => tool_metadata::glob::param::PATH_ABSOLUTE,
        PathMode::Allowed => tool_metadata::glob::param::PATH_ALLOWED,
    };
    let description = match glob {
        PathMode::Absolute => tool_metadata::glob::description::ABSOLUTE,
        PathMode::Allowed => tool_metadata::glob::description::ALLOWED,
    };

    definitions.push(tool_definition(
        tool_metadata::glob::NAME,
        description,
        object_schema(
            vec![
                (
                    tool_metadata::glob::param::PATTERN.name,
                    string_schema(tool_metadata::glob::param::PATTERN.description),
                ),
                (path.name, string_schema(path.description)),
            ],
            &[tool_metadata::glob::param::PATTERN.name, path.name],
        ),
    ));
}

fn push_grep_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    let Some(grep) = case.grep else {
        return;
    };

    let path = match grep.path_mode {
        PathMode::Absolute => tool_metadata::grep::param::PATH_ABSOLUTE,
        PathMode::Allowed => tool_metadata::grep::param::PATH_ALLOWED,
    };
    let description = match grep.path_mode {
        PathMode::Absolute => tool_metadata::grep::description::absolute(grep.line_numbers),
        PathMode::Allowed => tool_metadata::grep::description::allowed(grep.line_numbers),
    };

    definitions.push(tool_definition(
        tool_metadata::grep::NAME,
        description,
        object_schema(
            vec![
                (
                    tool_metadata::grep::param::PATTERN.name,
                    string_schema(tool_metadata::grep::param::PATTERN.description),
                ),
                (path.name, string_schema(path.description)),
                (
                    tool_metadata::grep::param::INCLUDE.name,
                    string_schema(tool_metadata::grep::param::INCLUDE.description),
                ),
                (
                    tool_metadata::grep::param::LIMIT.name,
                    integer_schema(
                        tool_metadata::grep::param::LIMIT.description,
                        Some(1),
                        Some(tool_metadata::grep::MAX_LIMIT as i64),
                    ),
                ),
            ],
            &[tool_metadata::grep::param::PATTERN.name, path.name],
        ),
    ));
}

fn push_webfetch_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    if !case.webfetch {
        return;
    }

    definitions.push(tool_definition(
        tool_metadata::webfetch::NAME,
        tool_metadata::webfetch::DESCRIPTION,
        object_schema(
            vec![
                (
                    tool_metadata::webfetch::param::URL.name,
                    string_schema(tool_metadata::webfetch::param::URL.description),
                ),
                (
                    tool_metadata::webfetch::param::TIMEOUT_MS.name,
                    integer_schema(
                        tool_metadata::webfetch::param::TIMEOUT_MS.description,
                        Some(1),
                        Some(tool_metadata::webfetch::MAX_TIMEOUT_MS as i64),
                    ),
                ),
            ],
            &[tool_metadata::webfetch::param::URL.name],
        ),
    ));
}

fn push_todo_write_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    if !case.todo_write {
        return;
    }

    definitions.push(tool_definition(
        tool_metadata::todo_write::NAME,
        tool_metadata::todo_write::DESCRIPTION,
        object_schema(
            vec![(
                tool_metadata::todo_write::param::TODOS.name,
                json!({
                    "type": "array",
                    "description": tool_metadata::todo_write::param::TODOS.description,
                    "items": {
                        "type": "object",
                        "properties": {
                            tool_metadata::todo_write::param::ID.name: string_schema(tool_metadata::todo_write::param::ID.description),
                            tool_metadata::todo_write::param::CONTENT.name: string_schema(tool_metadata::todo_write::param::CONTENT.description),
                            tool_metadata::todo_write::param::STATUS.name: enum_schema(tool_metadata::todo_write::param::STATUS.description, &["pending", "in_progress", "completed", "cancelled"]),
                            tool_metadata::todo_write::param::PRIORITY.name: enum_schema(tool_metadata::todo_write::param::PRIORITY.description, &["high", "medium", "low"]),
                        },
                        "required": [
                            tool_metadata::todo_write::param::ID.name,
                            tool_metadata::todo_write::param::CONTENT.name,
                            tool_metadata::todo_write::param::STATUS.name,
                            tool_metadata::todo_write::param::PRIORITY.name,
                        ],
                    },
                }),
            )],
            &[tool_metadata::todo_write::param::TODOS.name],
        ),
    ));
}

fn push_todo_read_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    if !case.todo_read {
        return;
    }

    definitions.push(tool_definition(
        tool_metadata::todo_read::NAME,
        tool_metadata::todo_read::DESCRIPTION,
        object_schema(Vec::new(), &[]),
    ));
}

fn push_task_definition(definitions: &mut Vec<Value>, case: PromptCase) {
    if case.task_targets.is_empty() {
        return;
    }

    definitions.push(tool_definition(
        tool_metadata::task::NAME,
        &task_description(case.task_targets),
        object_schema(
            vec![
                (
                    tool_metadata::task::param::DESCRIPTION.name,
                    string_schema(tool_metadata::task::param::DESCRIPTION.description),
                ),
                (
                    tool_metadata::task::param::PROMPT.name,
                    string_schema(tool_metadata::task::param::PROMPT.description),
                ),
                (
                    tool_metadata::task::param::SUBAGENT_TYPE.name,
                    string_schema(tool_metadata::task::param::SUBAGENT_TYPE.description),
                ),
                (
                    tool_metadata::task::param::COMMAND.name,
                    string_schema(tool_metadata::task::param::COMMAND.description),
                ),
            ],
            &[
                tool_metadata::task::param::DESCRIPTION.name,
                tool_metadata::task::param::PROMPT.name,
                tool_metadata::task::param::SUBAGENT_TYPE.name,
            ],
        ),
    ));
}

fn task_description(targets: &[TaskTarget]) -> String {
    let mut description = String::with_capacity(128 + targets.len() * 48);
    description.push_str(tool_metadata::task::DESCRIPTION_PREFIX);
    description.push_str("\n\nAvailable subagents:\n");
    for target in targets {
        description.push_str("- ");
        description.push_str(target.name);
        description.push_str(": ");
        description.push_str(target.description);
        description.push('\n');
    }
    description.truncate(description.trim_end().len());
    description
}

fn tool_definition(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "parameters": parameters,
    })
}

fn object_schema(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut props = Map::with_capacity(properties.len());
    for (name, schema) in properties {
        props.insert(name.to_string(), schema);
    }

    let mut object = Map::with_capacity(3);
    object.insert("type".to_string(), Value::String("object".to_string()));
    object.insert("properties".to_string(), Value::Object(props));
    if !required.is_empty() {
        object.insert(
            "required".to_string(),
            Value::Array(
                required
                    .iter()
                    .map(|name| Value::String((*name).to_string()))
                    .collect(),
            ),
        );
    }
    Value::Object(object)
}

fn string_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "description": description,
    })
}

fn string_schema_constrained(
    description: &str,
    min_length: Option<usize>,
    max_length: Option<usize>,
) -> Value {
    let mut schema = Map::with_capacity(4);
    schema.insert("type".to_string(), Value::String("string".to_string()));
    schema.insert(
        "description".to_string(),
        Value::String(description.to_string()),
    );
    if let Some(min) = min_length {
        schema.insert("minLength".to_string(), Value::from(min));
    }
    if let Some(max) = max_length {
        schema.insert("maxLength".to_string(), Value::from(max));
    }
    Value::Object(schema)
}

fn integer_schema(description: &str, minimum: Option<i64>, maximum: Option<i64>) -> Value {
    let mut schema = Map::with_capacity(4);
    schema.insert("type".to_string(), Value::String("integer".to_string()));
    schema.insert(
        "description".to_string(),
        Value::String(description.to_string()),
    );
    if let Some(min) = minimum {
        schema.insert("minimum".to_string(), Value::from(min));
    }
    if let Some(max) = maximum {
        schema.insert("maximum".to_string(), Value::from(max));
    }
    Value::Object(schema)
}

fn boolean_schema(description: &str) -> Value {
    json!({
        "type": "boolean",
        "description": description,
    })
}

fn enum_schema(description: &str, values: &[&str]) -> Value {
    json!({
        "type": "string",
        "description": description,
        "enum": values,
    })
}
