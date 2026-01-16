//! Type conversions between core types and serdesAI types.
//!
//! Provides [`From`] implementations and helper functions to bridge
//! [`llm_coding_tools_core`] types with serdesAI's tool system.

use llm_coding_tools_core::operations::EditError;
use llm_coding_tools_core::{ToolError as CoreError, ToolOutput, ToolResult as CoreResult};
use serde_json::json;
use serdes_ai::tools::{ToolError as SerdesError, ToolReturn};

/// Convert [`ToolOutput`] to [`ToolReturn`].
///
/// - Non-truncated output: `ToolReturn::text(content)`
/// - Truncated output: `ToolReturn::json({ "content": ..., "truncated": true })`
#[inline]
pub fn output_to_return(output: ToolOutput) -> ToolReturn {
    if output.truncated {
        ToolReturn::json(json!({
            "content": output.content,
            "truncated": true
        }))
    } else {
        ToolReturn::text(output.content)
    }
}

/// Convert core `ToolResult<ToolOutput>` to serdesAI `ToolResult`.
///
/// This is the primary conversion function for tool implementations.
/// Requires tool_name for proper error context in validation errors.
///
/// # Example
///
/// ```no_run
/// use llm_coding_tools_serdesai::convert::to_serdes_result;
/// use llm_coding_tools_core::{ToolOutput, ToolResult};
///
/// // In a tool implementation:
/// fn convert_result(core_result: ToolResult<ToolOutput>) -> serdes_ai::tools::ToolResult {
///     to_serdes_result("my_tool", core_result)
/// }
/// ```
#[inline]
pub fn to_serdes_result(
    tool_name: &str,
    result: CoreResult<ToolOutput>,
) -> Result<ToolReturn, SerdesError> {
    result
        .map(output_to_return)
        .map_err(|err| core_error_to_serdes(tool_name, err))
}

/// Convert [`EditError`] to serdesAI error.
///
/// Maps edit-specific errors to appropriate error types:
/// - Validation errors: `NotFound`, `AmbiguousMatch`, `EmptyOldString`, `IdenticalStrings`
/// - Execution errors: `Tool(ToolError)` (IO, path errors)
pub fn edit_error_to_serdes(err: EditError) -> SerdesError {
    match err {
        EditError::NotFound => SerdesError::validation_error(
            "edit",
            Some("old_string".to_string()),
            "old_string not found in file content".to_string(),
        ),
        EditError::AmbiguousMatch(count) => SerdesError::validation_error(
            "edit",
            Some("old_string".to_string()),
            format!(
                "oldString found {count} times and requires more code context to uniquely identify the intended match"
            ),
        ),
        EditError::EmptyOldString => SerdesError::validation_error(
            "edit",
            Some("old_string".to_string()),
            "old_string must not be empty".to_string(),
        ),
        EditError::IdenticalStrings => SerdesError::validation_error(
            "edit",
            None,
            "old_string and new_string must be different".to_string(),
        ),
        EditError::Tool(tool_err) => core_error_to_serdes("edit", tool_err),
    }
}

/// Convert core [`ToolError`] to serdesAI [`ToolError`] with tool name context.
pub(crate) fn core_error_to_serdes(tool_name: &str, err: CoreError) -> SerdesError {
    match &err {
        // Validation errors - input/parameter issues
        CoreError::InvalidPath(msg) => {
            SerdesError::validation_error(tool_name, Some("path".to_string()), msg.clone())
        }
        CoreError::InvalidPattern(msg) => {
            SerdesError::validation_error(tool_name, Some("pattern".to_string()), msg.clone())
        }
        CoreError::OutOfBounds(msg) => {
            SerdesError::validation_error(tool_name, Some("offset".to_string()), msg.clone())
        }
        CoreError::Validation(msg) => SerdesError::validation_error(tool_name, None, msg.clone()),
        // Execution errors - runtime failures
        CoreError::Io(_)
        | CoreError::Http(_)
        | CoreError::Execution(_)
        | CoreError::Timeout(_)
        | CoreError::Json(_) => SerdesError::execution_failed(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::{ToolError as CoreError, ToolOutput};

    #[test]
    fn tool_output_converts_to_text_when_not_truncated() {
        let output = ToolOutput::new("hello world");
        let ret = output_to_return(output);
        assert_eq!(ret.as_text(), Some("hello world"));
    }

    #[test]
    fn tool_output_converts_to_json_when_truncated() {
        let output = ToolOutput::truncated("partial content");
        let ret = output_to_return(output);
        let json = ret.as_json().expect("should be json");
        assert_eq!(json["content"], "partial content");
        assert_eq!(json["truncated"], true);
    }

    #[test]
    fn invalid_path_error_maps_to_validation_error() {
        let core_err = CoreError::InvalidPath("not absolute".into());
        let result: Result<ToolReturn, SerdesError> =
            Err(core_error_to_serdes("test_tool", core_err));
        let serdes_err = result.unwrap_err();
        // Use pattern matching - is_validation_error() doesn't exist
        assert!(matches!(serdes_err, SerdesError::ValidationFailed { .. }));
    }

    #[test]
    fn invalid_pattern_error_maps_to_validation_error() {
        let core_err = CoreError::InvalidPattern("bad regex".into());
        let result: Result<ToolReturn, SerdesError> =
            Err(core_error_to_serdes("test_tool", core_err));
        let serdes_err = result.unwrap_err();
        assert!(matches!(serdes_err, SerdesError::ValidationFailed { .. }));
    }

    #[test]
    fn out_of_bounds_error_maps_to_validation_error() {
        let core_err = CoreError::OutOfBounds("offset too large".into());
        let result: Result<ToolReturn, SerdesError> =
            Err(core_error_to_serdes("test_tool", core_err));
        let serdes_err = result.unwrap_err();
        assert!(matches!(serdes_err, SerdesError::ValidationFailed { .. }));
    }

    #[test]
    fn io_error_maps_to_execution_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let core_err: CoreError = io_err.into();
        let serdes_err = core_error_to_serdes("test_tool", core_err);
        // ExecutionFailed is not a ValidationFailed
        assert!(!matches!(serdes_err, SerdesError::ValidationFailed { .. }));
    }

    #[test]
    fn execution_error_maps_to_execution_failed() {
        let core_err = CoreError::Execution("command failed".into());
        let serdes_err = core_error_to_serdes("test_tool", core_err);
        assert!(!matches!(serdes_err, SerdesError::ValidationFailed { .. }));
        // Use message() which exists, and check the error content
        assert!(serdes_err.message().contains("execution error"));
    }

    #[test]
    fn timeout_error_maps_to_execution_failed() {
        let core_err = CoreError::Timeout("timed out".into());
        let serdes_err = core_error_to_serdes("test_tool", core_err);
        assert!(!matches!(serdes_err, SerdesError::ValidationFailed { .. }));
    }

    #[test]
    fn to_serdes_result_maps_success() {
        let core_result: CoreResult<ToolOutput> = Ok(ToolOutput::new("success"));
        let serdes_result = to_serdes_result("test_tool", core_result);
        assert!(serdes_result.is_ok());
        assert_eq!(serdes_result.unwrap().as_text(), Some("success"));
    }

    #[test]
    fn to_serdes_result_maps_error() {
        let core_result: CoreResult<ToolOutput> =
            Err(CoreError::Execution("command failed".into()));
        let serdes_result = to_serdes_result("test_tool", core_result);
        assert!(serdes_result.is_err());
    }

    #[test]
    fn to_serdes_result_includes_tool_name_in_validation_error() {
        let core_result: CoreResult<ToolOutput> = Err(CoreError::InvalidPath("bad path".into()));
        let serdes_result = to_serdes_result("read_file", core_result);
        let err = serdes_result.unwrap_err();
        assert!(matches!(err, SerdesError::ValidationFailed { .. }));
        // Validation error should include the error details
        match err {
            SerdesError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "read_file");
                assert!(!errors.is_empty());
                assert!(errors[0].message.contains("bad path"));
            }
            _ => panic!("Expected ValidationFailed"),
        }
    }
}
