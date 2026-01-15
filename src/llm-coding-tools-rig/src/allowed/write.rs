//! Write file tool using [`AllowedPathResolver`].

use llm_coding_tools_core::operations::write_file;
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::{ToolContext, ToolError, ToolResult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Arguments for the write tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WriteToolArgs {
    /// Relative path for the file to write (within allowed directories).
    pub file_path: String,
    /// Content to write to the file.
    pub content: String,
}

/// Tool for writing content to files within allowed directories.
#[derive(Debug, Clone)]
pub struct WriteTool {
    resolver: AllowedPathResolver,
}

impl WriteTool {
    /// Creates a new write tool restricted to the given directories.
    pub fn new(allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> ToolResult<Self> {
        let paths: Vec<PathBuf> = allowed_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        Ok(Self {
            resolver: AllowedPathResolver::new(paths)?,
        })
    }

    /// Creates a new write tool with an existing resolver.
    pub fn with_resolver(resolver: AllowedPathResolver) -> Self {
        Self { resolver }
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
            description: "Write content to a file within allowed directories. \
                          Paths are relative to configured base directories."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(WriteToolArgs))
                .expect("schema generation should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        write_file(&self.resolver, &args.file_path, &args.content).await
    }
}

impl ToolContext for WriteTool {
    const NAME: &'static str = "write";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::WRITE_ALLOWED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn writes_new_file() {
        let dir = TempDir::new().unwrap();
        let tool = WriteTool::new([dir.path()]).unwrap();
        let result = tool
            .call(WriteToolArgs {
                file_path: "new.txt".to_string(),
                content: "hello".to_string(),
            })
            .await
            .unwrap();
        assert!(result.contains("5 bytes"));
        assert!(dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool = WriteTool::new([dir.path()]).unwrap();
        let result = tool
            .call(WriteToolArgs {
                file_path: "../../../tmp/escape.txt".to_string(),
                content: "content".to_string(),
            })
            .await;
        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }
}
