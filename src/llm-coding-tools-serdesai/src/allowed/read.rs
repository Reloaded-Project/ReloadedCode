//! Read file tool using [`AllowedPathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::operations::read_file;
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::{ToolContext, ToolResult as CoreToolResult};
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::path::PathBuf;

use crate::convert::to_serdes_result;

const DEFAULT_OFFSET: usize = 1;
const DEFAULT_LIMIT: usize = 2000;

fn default_offset() -> usize {
    DEFAULT_OFFSET
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct ReadArgs {
    /// Path to the file (relative to allowed directories).
    file_path: String,
    /// Line offset to start reading from (1-based). Defaults to 1.
    #[serde(default = "default_offset")]
    offset: usize,
    /// Maximum number of lines to return. Defaults to 2000.
    #[serde(default = "default_limit")]
    limit: usize,
}

/// Tool for reading file contents with optional line numbers.
///
/// Restricts access to configured allowed directories.
#[derive(Debug, Clone)]
pub struct ReadTool<const LINE_NUMBERS: bool = true> {
    resolver: AllowedPathResolver,
}

impl<const LINE_NUMBERS: bool> ReadTool<LINE_NUMBERS> {
    /// Creates a new read tool restricted to the given directories.
    ///
    /// Returns an error if any directory doesn't exist or can't be canonicalized.
    pub fn new(allowed_directories: Vec<PathBuf>) -> CoreToolResult<Self> {
        Ok(Self {
            resolver: AllowedPathResolver::new(allowed_directories)?,
        })
    }
}

#[async_trait]
impl<Deps: Send + Sync, const LINE_NUMBERS: bool> Tool<Deps> for ReadTool<LINE_NUMBERS> {
    fn definition(&self) -> ToolDefinition {
        let description = if LINE_NUMBERS {
            "Read file contents with line numbers from allowed directories. \
             Paths are relative to configured base directories."
        } else {
            "Read file contents from allowed directories. \
             Paths are relative to configured base directories."
        };
        let schema = SchemaBuilder::new()
            .string(
                "file_path",
                "Path to the file (relative to allowed directories)",
                true,
            )
            .integer_constrained(
                "offset",
                "Line offset to start reading from (1-based). Defaults to 1.",
                false,
                Some(1),
                None,
            )
            .integer_constrained(
                "limit",
                "Maximum number of lines to return. Defaults to 2000.",
                false,
                Some(1),
                None,
            )
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new("Read", description).with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: ReadArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("Read", None, e.to_string()))?;

        let result =
            read_file::<_, LINE_NUMBERS>(&self.resolver, &args.file_path, args.offset, args.limit)
                .await;
        to_serdes_result("Read", result)
    }
}

impl<const LINE_NUMBERS: bool> ToolContext for ReadTool<LINE_NUMBERS> {
    const NAME: &'static str = "Read";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::READ_ALLOWED
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
    async fn reads_file_with_line_numbers() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello\nworld\n").unwrap();

        let tool: ReadTool<true> = ReadTool::new(vec![dir.path().to_path_buf()]).unwrap();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "test.txt",
                    "offset": 1,
                    "limit": 2000
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("L1: hello"));
        assert!(text.contains("L2: world"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool: ReadTool<true> = ReadTool::new(vec![dir.path().to_path_buf()]).unwrap();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "../../../etc/passwd"
                }),
            )
            .await;

        assert!(result.is_err());
    }
}
