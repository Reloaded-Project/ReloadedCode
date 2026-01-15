//! Read file tool using [`AllowedPathResolver`].

use llm_coding_tools_core::operations::read_file;
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::{ToolContext, ToolError, ToolOutput, ToolResult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::path::{Path, PathBuf};

const DEFAULT_OFFSET: usize = 1;
const DEFAULT_LIMIT: usize = 2000;

fn default_offset() -> usize {
    DEFAULT_OFFSET
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// Arguments for the read file tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Relative path to the file to read (within allowed directories).
    pub file_path: String,
    /// 1-indexed line number to start reading from (default: 1).
    #[serde(default = "default_offset")]
    pub offset: usize,
    /// Maximum number of lines to return (default: 2000).
    #[serde(default = "default_limit")]
    pub limit: usize,
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
    pub fn new(allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> ToolResult<Self> {
        let paths: Vec<PathBuf> = allowed_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        Ok(Self {
            resolver: AllowedPathResolver::new(paths)?,
        })
    }

    /// Creates a new read tool with an existing resolver.
    pub fn with_resolver(resolver: AllowedPathResolver) -> Self {
        Self { resolver }
    }
}

impl<const LINE_NUMBERS: bool> Tool for ReadTool<LINE_NUMBERS> {
    const NAME: &'static str = "read";

    type Error = ToolError;
    type Args = ReadArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let description = if LINE_NUMBERS {
            "Read file contents with line numbers from allowed directories. \
             Paths are relative to configured base directories."
        } else {
            "Read file contents from allowed directories. \
             Paths are relative to configured base directories."
        };
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: description.to_string(),
            parameters: serde_json::to_value(schema_for!(ReadArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        read_file::<_, LINE_NUMBERS>(&self.resolver, &args.file_path, args.offset, args.limit).await
    }
}

impl<const LINE_NUMBERS: bool> ToolContext for ReadTool<LINE_NUMBERS> {
    const NAME: &'static str = "read";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::READ_ALLOWED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn reads_file_with_line_numbers() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello\nworld\n").unwrap();

        let tool: ReadTool<true> = ReadTool::new([dir.path()]).unwrap();
        let args = ReadArgs {
            file_path: "test.txt".to_string(),
            offset: 1,
            limit: 2000,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result.content, "L1: hello\nL2: world");
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool: ReadTool = ReadTool::new([dir.path()]).unwrap();
        let args = ReadArgs {
            file_path: "../../../etc/passwd".to_string(),
            offset: 1,
            limit: 100,
        };
        let result = tool.call(args).await;
        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }
}
