//! Grep content search tool using [`AllowedPathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::operations::{DEFAULT_MAX_LINE_LENGTH, grep_search};
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::tool_names;
use serde::Deserialize;
use serdes_ai::tools::{
    RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult, ToolReturn,
};
use std::path::Path;

use crate::convert::to_serdes_result;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2000;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct GrepArgs {
    /// Regular expression pattern to search for in file contents.
    pattern: String,
    /// Directory path to search in (relative to allowed directories).
    path: String,
    /// File pattern to filter search results (e.g., "*.rs", "*.{ts,tsx}").
    #[serde(default)]
    include: Option<String>,
    /// Maximum number of matches to return (default: 100, max: 2000).
    #[serde(default)]
    limit: Option<usize>,
}

/// Tool for searching file contents within allowed directories.
#[derive(Debug, Clone)]
pub struct GrepTool<const LINE_NUMBERS: bool = true> {
    resolver: AllowedPathResolver,
}

impl<const LINE_NUMBERS: bool> GrepTool<LINE_NUMBERS> {
    /// Creates a new grep tool restricted to the given directories.
    pub fn new(
        allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> llm_coding_tools_core::ToolResult<Self> {
        Ok(Self {
            resolver: AllowedPathResolver::new(allowed_paths)?,
        })
    }
}

#[async_trait]
impl<Deps: Send + Sync, const LINE_NUMBERS: bool> Tool<Deps> for GrepTool<LINE_NUMBERS> {
    fn definition(&self) -> ToolDefinition {
        let description = if LINE_NUMBERS {
            "Search file contents using regex patterns within allowed directories. \
             Returns matches with line numbers. Paths are relative to configured base directories."
        } else {
            "Search file contents using regex patterns within allowed directories. \
             Paths are relative to configured base directories."
        };
        let schema = SchemaBuilder::new()
            .string(
                "pattern",
                "Regular expression pattern to search for in file contents",
                true,
            )
            .string(
                "path",
                "Directory path to search in (relative to allowed directories)",
                true,
            )
            .string(
                "include",
                "File pattern to filter search results (e.g., \"*.rs\", \"*.{ts,tsx}\")",
                false,
            )
            .integer_constrained(
                "limit",
                "Maximum number of matches to return (default: 100, max: 2000)",
                false,
                Some(1),
                Some(2000),
            )
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new(tool_names::GREP, description).with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: GrepArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::GREP, None, e.to_string()))?;

        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Err(ToolError::validation_error(
                tool_names::GREP,
                Some("pattern".to_string()),
                "pattern must not be empty".to_string(),
            ));
        }

        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        if limit == 0 {
            return Err(ToolError::validation_error(
                tool_names::GREP,
                Some("limit".to_string()),
                "limit must be greater than zero".to_string(),
            ));
        }

        let include = args.include.as_deref().and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });

        let result = grep_search(&self.resolver, pattern, include, &args.path, limit);

        match result {
            Err(e) => to_serdes_result(tool_names::GREP, Err(e)),
            Ok(grep_output) => {
                if grep_output.files.is_empty() {
                    return Ok(ToolReturn::text("No matches found."));
                }

                let output = grep_output.format::<LINE_NUMBERS>(limit, DEFAULT_MAX_LINE_LENGTH);
                Ok(ToolReturn::text(output))
            }
        }
    }
}

impl<const LINE_NUMBERS: bool> ToolContext for GrepTool<LINE_NUMBERS> {
    const NAME: &'static str = tool_names::GREP;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::GREP_ALLOWED
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
    async fn finds_matching_content() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let tool: GrepTool<true> = GrepTool::new([dir.path()]).unwrap();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "hello",
                    "path": "."
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("Found 1 matches"));
        assert!(text.contains("L1: hello world"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool: GrepTool<true> = GrepTool::new([dir.path()]).unwrap();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "test",
                    "path": "../../../etc"
                }),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_empty_pattern() {
        let dir = TempDir::new().unwrap();
        let tool: GrepTool<true> = GrepTool::new([dir.path()]).unwrap();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "   ",
                    "path": "."
                }),
            )
            .await;

        assert!(result.is_err());
    }
}
