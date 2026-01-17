//! Edit file tool using [`AbsolutePathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::operations::edit_file;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::tool_names;
use serde::Deserialize;
use serdes_ai::tools::{
    RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult, ToolReturn,
};

use crate::convert::edit_error_to_serdes;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct EditArgs {
    /// Absolute path to the file.
    file_path: String,
    /// The exact text to find and replace.
    old_string: String,
    /// The text to replace with.
    new_string: String,
    /// Replace all occurrences instead of just the first. Defaults to false.
    #[serde(default)]
    replace_all: bool,
}

/// Tool for making exact string replacements in files.
#[derive(Debug, Clone, Default)]
pub struct EditTool;

impl EditTool {
    /// Creates a new edit tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for EditTool {
    fn definition(&self) -> ToolDefinition {
        let schema = SchemaBuilder::new()
            .string("file_path", "Absolute path to the file", true)
            .string("old_string", "The exact text to find and replace", true)
            .string("new_string", "The text to replace with", true)
            .boolean(
                "replace_all",
                "Replace all occurrences instead of just the first. Defaults to false.",
                false,
            )
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new(
             tool_names::EDIT,
             "Makes exact string replacements in files. Use replace_all=true to replace all occurrences.",
         )
         .with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: EditArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::EDIT, None, e.to_string()))?;

        let resolver = AbsolutePathResolver;
        let result = edit_file(
            &resolver,
            &args.file_path,
            &args.old_string,
            &args.new_string,
            args.replace_all,
        )
        .await;

        result.map(ToolReturn::text).map_err(edit_error_to_serdes)
    }
}

impl ToolContext for EditTool {
    const NAME: &'static str = tool_names::EDIT;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::EDIT_ABSOLUTE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn edit_success() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let tool = EditTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file.path().to_string_lossy(),
                    "old_string": "world",
                    "new_string": "rust"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("1 occurrence"));
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "hello rust");
    }

    #[tokio::test]
    async fn edit_not_found_error() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let tool = EditTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file.path().to_string_lossy(),
                    "old_string": "not_found",
                    "new_string": "replacement"
                }),
            )
            .await;

        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ValidationFailed { .. }));
        // Check the error contains the validation message
        match err {
            ToolError::ValidationFailed { errors, .. } => {
                assert!(!errors.is_empty());
                assert!(errors[0].message.contains("not found"));
            }
            _ => panic!("Expected ValidationFailed"),
        }
    }

    #[tokio::test]
    async fn edit_ambiguous_match_error() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello hello hello").unwrap();
        file.flush().unwrap();

        let tool = EditTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file.path().to_string_lossy(),
                    "old_string": "hello",
                    "new_string": "world",
                    "replace_all": false
                }),
            )
            .await;

        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ValidationFailed { .. }));
        // Check the error contains the validation message
        match err {
            ToolError::ValidationFailed { errors, .. } => {
                assert!(!errors.is_empty());
                assert!(errors[0].message.contains("3 times"));
            }
            _ => panic!("Expected ValidationFailed"),
        }
    }
}
