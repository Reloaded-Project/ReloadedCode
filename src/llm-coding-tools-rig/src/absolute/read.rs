//! Read file tool using [`AbsolutePathResolver`].

use llm_coding_tools_core::operations::read_file;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::{ToolContext, ToolError, ToolOutput};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

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
    /// Absolute path to the file to read.
    pub file_path: String,
    /// 1-indexed line number to start reading from (default: 1).
    #[serde(default = "default_offset")]
    pub offset: usize,
    /// Maximum number of lines to return (default: 2000).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Tool for reading file contents with optional line numbers.
#[derive(Debug, Clone, Default)]
pub struct ReadTool<const LINE_NUMBERS: bool = true>;

impl<const LINE_NUMBERS: bool> ReadTool<LINE_NUMBERS> {
    /// Creates a new read tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl<const LINE_NUMBERS: bool> Tool for ReadTool<LINE_NUMBERS> {
    const NAME: &'static str = tool_names::READ;

    type Error = ToolError;
    type Args = ReadArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let description = if LINE_NUMBERS {
            "Read file contents with line numbers. Returns lines prefixed with L{number}: format."
        } else {
            "Read file contents. Returns raw file content without line number prefixes."
        };
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: description.to_string(),
            parameters: serde_json::to_value(schema_for!(ReadArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resolver = AbsolutePathResolver;
        read_file::<_, LINE_NUMBERS>(&resolver, &args.file_path, args.offset, args.limit).await
    }
}

impl<const LINE_NUMBERS: bool> ToolContext for ReadTool<LINE_NUMBERS> {
    const NAME: &'static str = tool_names::READ;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::READ_ABSOLUTE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn reads_file_with_line_numbers() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"hello\nworld\n").unwrap();
        let tool: ReadTool<true> = ReadTool::new();
        let args = ReadArgs {
            file_path: temp.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2000,
        };
        let result = tool.call(args).await.unwrap();
        assert_eq!(result.content, "L1: hello\nL2: world");
    }

    #[tokio::test]
    async fn rejects_relative_path() {
        let tool: ReadTool = ReadTool::new();
        let args = ReadArgs {
            file_path: "relative/path.txt".to_string(),
            offset: 1,
            limit: 100,
        };
        let result = tool.call(args).await;
        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }
}
