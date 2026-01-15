//! Write file tool using [`AbsolutePathResolver`].

use llm_coding_tools_core::operations::write_file;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::{ToolContext, ToolError};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

/// Arguments for the write tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteToolArgs {
    /// Absolute path for the file to write.
    pub file_path: String,
    /// Content to write to the file.
    pub content: String,
}

/// Tool for writing content to files.
#[derive(Debug, Clone, Default)]
pub struct WriteTool;

impl WriteTool {
    /// Creates a new write tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Tool for WriteTool {
    const NAME: &'static str = "write";

    type Error = ToolError;
    type Args = WriteToolArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Write content to a file, creating parent directories if needed. \
                           Overwrites existing files."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(WriteToolArgs))
                .expect("schema generation should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resolver = AbsolutePathResolver;
        write_file(&resolver, &args.file_path, &args.content).await
    }
}

impl ToolContext for WriteTool {
    const NAME: &'static str = "write";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::WRITE_ABSOLUTE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn writes_new_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("new.txt");
        let tool = WriteTool::new();
        let result = tool
            .call(WriteToolArgs {
                file_path: file_path.to_string_lossy().to_string(),
                content: "hello".to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("5 bytes"));
    }

    #[tokio::test]
    async fn rejects_relative_path() {
        let tool = WriteTool::new();
        let result = tool
            .call(WriteToolArgs {
                file_path: "relative/path.txt".to_string(),
                content: "content".to_string(),
            })
            .await;
        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }
}
