//! Type conversions between core types and serdesAI types.
//!
//! Provides [`From`] implementations and helper functions to bridge
//! [`llm_coding_tools_core`] types with serdesAI's tool system.
//!
//! [`llm_coding_tools_core`]: llm_coding_tools_core

use llm_coding_tools_core::{ToolError as CoreError, ToolOutput, ToolResult as CoreResult};
use serde_json::json;
use serdes_ai::tools::{ToolError as SerdesError, ToolReturn};

/// Convert [`ToolOutput`] to [`ToolReturn`] (serdesAI).
///
/// - Non-truncated output: `ToolReturn::text(content)`
/// - Truncated output: `ToolReturn::json({ "content": ..., "truncated": true })`
///
/// [`ToolOutput`]: llm_coding_tools_core::ToolOutput
/// [`ToolReturn`]: serdes_ai::tools::ToolReturn
#[inline]
fn output_to_return(output: ToolOutput) -> ToolReturn {
    if output.truncated {
        ToolReturn::json(json!({
            "content": output.content,
            "truncated": true
        }))
    } else {
        ToolReturn::text(output.content)
    }
}

/// Convert core [`ToolResult<ToolOutput>`] to serdesAI [`ToolResult`].
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
///
/// [`ToolResult<ToolOutput>`]: llm_coding_tools_core::ToolResult
/// [`ToolResult`]: serdes_ai::tools::ToolResult
///
/// # Errors
/// - Returns [`SerdesError`] when the core [`ToolResult`] contains a [`ToolError`],
///   converted via [`core_error_to_serdes`].
#[inline]
pub fn to_serdes_result(
    tool_name: &str,
    result: CoreResult<ToolOutput>,
) -> Result<ToolReturn, SerdesError> {
    result
        .map(output_to_return)
        .map_err(|err| core_error_to_serdes(tool_name, err))
}

/// Convert core [`ToolError`][core] to serdesAI [`ToolError`][serdes] with tool name context.
///
/// [core]: llm_coding_tools_core::ToolError
/// [serdes]: serdes_ai::tools::ToolError
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
            SerdesError::validation_error(tool_name, field_for_out_of_bounds(msg), msg.clone())
        }
        CoreError::Validation { field, message } => {
            SerdesError::validation_error(tool_name, field.clone(), message.clone())
        }
        CoreError::Json(msg) => SerdesError::validation_error(tool_name, None, msg.to_string()),
        // Permission denied - runtime failure (policy/permission issue)
        CoreError::PermissionDenied { tool, subject } => SerdesError::execution_failed(format!(
            "Permission denied for tool '{}' on subject '{}'",
            tool, subject
        )),
        // Execution errors - runtime failures
        CoreError::Io(_)
        | CoreError::Http(_)
        | CoreError::Execution(_)
        | CoreError::Timeout(_)
        | CoreError::TimeoutWithKillFailure { .. } => {
            SerdesError::execution_failed(err.to_string())
        }
    }
}

fn field_for_out_of_bounds(msg: &str) -> Option<String> {
    if msg.starts_with("offset ") || msg.starts_with("offset must") {
        Some("offset".to_string())
    } else if msg.starts_with("limit ") || msg.starts_with("limit must") {
        Some("limit".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::{ToolError as CoreError, ToolOutput};
    use rstest::rstest;

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

    #[rstest]
    #[case::invalid_path(CoreError::InvalidPath("not absolute".into()), "test_tool")]
    #[case::invalid_pattern(CoreError::InvalidPattern("bad regex".into()), "test_tool")]
    #[case::out_of_bounds(CoreError::OutOfBounds("offset too large".into()), "test_tool")]
    #[case::json(serde_json::from_str::<serde_json::Value>("{").unwrap_err().into(), "test_tool")]
    fn validation_errors_map_to_validation_failed(
        #[case] core_err: CoreError,
        #[case] tool_name: &str,
    ) {
        let serdes_err = core_error_to_serdes(tool_name, core_err);
        assert!(matches!(serdes_err, SerdesError::ValidationFailed { .. }));
    }

    #[test]
    fn to_serdes_result_preserves_tool_name_in_validation_errors() {
        let core_result: CoreResult<ToolOutput> = Err(CoreError::InvalidPath("bad path".into()));
        let serdes_err = to_serdes_result("read_file", core_result).unwrap_err();

        assert!(matches!(serdes_err, SerdesError::ValidationFailed { .. }));
        match serdes_err {
            SerdesError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "read_file");
                assert!(!errors.is_empty());
                assert!(errors[0].message.contains("bad path"));
            }
            _ => unreachable!(),
        }
    }

    /// Ensure execution/runtime errors (Io, Execution, Timeout, etc.) are NOT
    /// converted to validation errors. They must stay on the runtime failure path.
    #[rstest]
    #[case::io_error(
        CoreError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "file not found")),
        None,
        None
    )]
    #[case::execution_error(
        CoreError::Execution("command failed".into()),
        Some("execution error"),
        None,
    )]
    #[case::timeout(CoreError::Timeout("timed out".into()), None, None)]
    #[case::timeout_with_kill_failure(
        CoreError::TimeoutWithKillFailure {
            message: "timed out".into(),
            kill_error: "operation not permitted".into(),
        },
        Some("timed out"),
        Some("operation not permitted"),
    )]
    fn execution_errors_stay_on_runtime_failure_path(
        #[case] core_err: CoreError,
        #[case] expected_msg: Option<&str>,
        #[case] expected_extra: Option<&str>,
    ) {
        let serdes_err = core_error_to_serdes("test_tool", core_err);
        let is_validation_error = matches!(serdes_err, SerdesError::ValidationFailed { .. });
        assert!(
            !is_validation_error,
            "execution errors must not be validation errors"
        );
        if let Some(msg) = expected_msg {
            assert!(serdes_err.message().contains(msg));
        }
        if let Some(msg) = expected_extra {
            assert!(serdes_err.message().contains(msg));
        }
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
    fn out_of_bounds_limit_maps_to_limit_field() {
        let serdes_err =
            core_error_to_serdes("read", CoreError::OutOfBounds("limit must be >= 1".into()));

        match serdes_err {
            SerdesError::ValidationFailed { errors, .. } => {
                assert_eq!(errors[0].field.as_deref(), Some("limit"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn validation_old_string_maps_to_old_string_field() {
        let serdes_err = core_error_to_serdes(
            "edit",
            CoreError::validation_for("old_string", "old_string must not be empty"),
        );

        match serdes_err {
            SerdesError::ValidationFailed { errors, .. } => {
                assert_eq!(errors[0].field.as_deref(), Some("old_string"));
            }
            _ => unreachable!(),
        }
    }
}
