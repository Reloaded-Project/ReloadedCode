//! Shared helpers for Edit tool implementations.

use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::tools::EditError;
use serdes_ai::tools::ToolError;

use crate::convert::core_error_to_serdes;

/// Convert [`EditError`] to serdesAI error.
///
/// Maps edit-specific errors to appropriate error types:
/// - Validation errors: `NotFound`, `AmbiguousMatch`, `EmptyOldString`, `IdenticalStrings`
/// - Execution errors: `Tool(ToolError)` (IO, path errors)
pub(crate) fn error_to_serdes(err: EditError) -> ToolError {
    match err {
        EditError::NotFound => ToolError::validation_error(
            tool_names::EDIT,
            Some("old_string".to_string()),
            "old_string not found in file content".to_string(),
        ),
        EditError::AmbiguousMatch(count) => ToolError::validation_error(
            tool_names::EDIT,
            Some("old_string".to_string()),
            format!(
                "old_string found {count} times and requires more code context to uniquely identify the intended match"
            ),
        ),
        EditError::EmptyOldString => ToolError::validation_error(
            tool_names::EDIT,
            Some("old_string".to_string()),
            "old_string must not be empty".to_string(),
        ),
        EditError::IdenticalStrings => ToolError::validation_error(
            tool_names::EDIT,
            None,
            "old_string and new_string must be different".to_string(),
        ),
        EditError::Tool(tool_err) => core_error_to_serdes(tool_names::EDIT, tool_err),
    }
}
