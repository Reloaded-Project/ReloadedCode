//! Write file tool using [`AbsolutePathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::operations::write_file;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::{ToolContext, ToolOutput};
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};

use crate::convert::to_serdes_result;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct WriteArgs {
    /// Absolute path to the file.
    file_path: String,
    /// Content to write to the file.
    content: String,
}

/// Tool for writing content to files.
///
/// Creates parent directories if needed and overwrites existing files.
#[derive(Debug, Clone, Default)]
pub struct WriteTool;

impl WriteTool {
    /// Creates a new write tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for WriteTool {
    fn definition(&self) -> ToolDefinition {
        let schema = SchemaBuilder::new()
            .string("file_path", "Absolute path to the file", true)
            .string("content", "Content to write to the file", true)
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new(
            tool_names::WRITE,
            "Write content to a file, creating parent directories if needed. Overwrites existing files.",
        )
        .with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: WriteArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::WRITE, None, e.to_string()))?;

        let resolver = AbsolutePathResolver;
        let result = write_file(&resolver, &args.file_path, &args.content).await;

        // Convert String result to ToolOutput for consistent error handling
        to_serdes_result(tool_names::WRITE, result.map(ToolOutput::new))
    }
}

impl ToolContext for WriteTool {
    const NAME: &'static str = tool_names::WRITE;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::WRITE_ABSOLUTE
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
    async fn writes_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("new.txt");
        let tool = WriteTool::new();

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "content": "hello world"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("11 bytes"));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
    }
}
