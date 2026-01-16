//! Edit file tool using [`AllowedPathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::operations::edit_file;
use llm_coding_tools_core::path::AllowedPathResolver;
use serde::Deserialize;
use serdes_ai::tools::{
    RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult, ToolReturn,
};
use std::path::PathBuf;

use crate::convert::edit_error_to_serdes;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct EditArgs {
    /// Path to the file (relative to allowed directories).
    file_path: String,
    /// The exact text to find and replace.
    old_string: String,
    /// The text to replace with.
    new_string: String,
    /// Replace all occurrences instead of just the first. Defaults to false.
    #[serde(default)]
    replace_all: bool,
}

/// Tool for making exact string replacements in files within allowed directories.
#[derive(Debug, Clone)]
pub struct EditTool {
    resolver: AllowedPathResolver,
}

impl EditTool {
    /// Creates a new edit tool restricted to the given canonical directories.
    /// `allowed_directories` should already be canonicalized (see [`AllowedPathResolver::from_canonical`]).
    pub fn new(allowed_directories: Vec<PathBuf>) -> Self {
        Self {
            resolver: AllowedPathResolver::from_canonical(allowed_directories),
        }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for EditTool {
    fn definition(&self) -> ToolDefinition {
        let schema = SchemaBuilder::new()
            .string(
                "file_path",
                "Path to the file (relative to allowed directories)",
                true,
            )
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
            "Edit",
            "Make exact string replacements in files within allowed directories. \
             Paths are relative to configured base directories.",
        )
        .with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: EditArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("Edit", None, e.to_string()))?;

        let result = edit_file(
            &self.resolver,
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
    const NAME: &'static str = "Edit";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::EDIT_ALLOWED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use tempfile::TempDir;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn replaces_single_occurrence() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let tool = EditTool::new(vec![dir.path().to_path_buf()]);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "test.txt",
                    "old_string": "world",
                    "new_string": "rust"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("1 occurrence"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool = EditTool::new(vec![dir.path().to_path_buf()]);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "../../../etc/passwd",
                    "old_string": "old",
                    "new_string": "new"
                }),
            )
            .await;

        assert!(result.is_err());
    }
}
