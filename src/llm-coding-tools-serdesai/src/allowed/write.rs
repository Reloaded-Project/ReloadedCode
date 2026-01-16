//! Write file tool using [`AllowedPathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::operations::write_file;
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::{ToolContext, ToolOutput};
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::path::PathBuf;

use crate::convert::to_serdes_result;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct WriteArgs {
    /// Path to the file (relative to allowed directories).
    file_path: String,
    content: String,
}

/// Tool for writing content to files within allowed directories.
#[derive(Debug, Clone)]
pub struct WriteTool {
    resolver: AllowedPathResolver,
}

impl WriteTool {
    /// Creates a new write tool restricted to the given directories.
    pub fn new(allowed_directories: Vec<PathBuf>) -> Self {
        Self {
            resolver: AllowedPathResolver::from_canonical(allowed_directories),
        }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for WriteTool {
    fn definition(&self) -> ToolDefinition {
        let schema = SchemaBuilder::new()
            .string(
                "file_path",
                "Path to the file (relative to allowed directories)",
                true,
            )
            .string("content", "Content to write to the file", true)
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new(
            "Write",
            "Write content to a file within allowed directories. \
             Paths are relative to configured base directories.",
        )
        .with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: WriteArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("Write", None, e.to_string()))?;

        let result = write_file(&self.resolver, &args.file_path, &args.content).await;
        to_serdes_result("Write", result.map(ToolOutput::new))
    }
}

impl ToolContext for WriteTool {
    const NAME: &'static str = "Write";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::WRITE_ALLOWED
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
    async fn writes_new_file() {
        let dir = TempDir::new().unwrap();
        let tool = WriteTool::new(vec![dir.path().to_path_buf()]);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "new.txt",
                    "content": "hello"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("5 bytes"));
        assert!(dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool = WriteTool::new(vec![dir.path().to_path_buf()]);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "../../../tmp/escape.txt",
                    "content": "content"
                }),
            )
            .await;

        assert!(result.is_err());
    }
}
