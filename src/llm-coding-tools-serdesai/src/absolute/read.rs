//! Read file tool using [`AbsolutePathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::operations::read_file;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::tool_names;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};

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
    /// Absolute path to the file.
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
/// The `LINE_NUMBERS` const generic controls output format:
/// - `true` (default): Lines prefixed with `L{number}: `
/// - `false`: Raw file content
#[derive(Debug, Clone, Default)]
pub struct ReadTool<const LINE_NUMBERS: bool = true>;

impl<const LINE_NUMBERS: bool> ReadTool<LINE_NUMBERS> {
    /// Creates a new read tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl<Deps: Send + Sync, const LINE_NUMBERS: bool> Tool<Deps> for ReadTool<LINE_NUMBERS> {
    fn definition(&self) -> ToolDefinition {
        let description = if LINE_NUMBERS {
            "Read file contents with line numbers. Returns lines prefixed with L{number}: format."
        } else {
            "Read file contents. Returns raw file content without line number prefixes."
        };
        let schema = SchemaBuilder::new()
            .string("file_path", "Absolute path to the file", true)
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

        ToolDefinition::new(tool_names::READ, description).with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: ReadArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::READ, None, e.to_string()))?;

        let resolver = AbsolutePathResolver;
        // Core uses 1-indexed offset directly; args.offset defaults to 1
        let result =
            read_file::<_, LINE_NUMBERS>(&resolver, &args.file_path, args.offset, args.limit).await;
        to_serdes_result(tool_names::READ, result)
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
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn reads_file_with_offset_and_limit() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"line1\nline2\nline3\nline4\n").unwrap();
        let tool: ReadTool<true> = ReadTool::new();

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": temp.path().to_string_lossy(),
                    "offset": 2,
                    "limit": 2
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("L2: line2"));
        assert!(text.contains("L3: line3"));
        assert!(!text.contains("L1:"));
        assert!(!text.contains("L4:"));
    }
}
