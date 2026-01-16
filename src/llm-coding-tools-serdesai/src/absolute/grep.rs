//! Grep content search tool using [`AbsolutePathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::operations::grep_search;
use llm_coding_tools_core::path::AbsolutePathResolver;
use serde::Deserialize;
use serdes_ai::tools::{
    RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult, ToolReturn,
};
use std::fmt::Write;

use crate::convert::to_serdes_result;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2000;
const MAX_LINE_LENGTH: usize = 2000;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct GrepArgs {
    /// Regular expression pattern to search for in file contents.
    pattern: String,
    /// Absolute directory path to search in.
    path: String,
    /// File pattern to filter search results (e.g., "*.rs", "*.{ts,tsx}").
    #[serde(default)]
    include: Option<String>,
    /// Maximum number of matches to return (default: 100, max: 2000).
    #[serde(default)]
    limit: Option<usize>,
}

/// Tool for searching file contents using regex patterns.
///
/// The `LINE_NUMBERS` const generic controls output format:
/// - `true` (default): Lines prefixed with `L{number}: `
/// - `false`: Raw matching lines
#[derive(Debug, Clone, Default)]
pub struct GrepTool<const LINE_NUMBERS: bool = true>;

impl<const LINE_NUMBERS: bool> GrepTool<LINE_NUMBERS> {
    /// Creates a new grep tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl<Deps: Send + Sync, const LINE_NUMBERS: bool> Tool<Deps> for GrepTool<LINE_NUMBERS> {
    fn definition(&self) -> ToolDefinition {
        // Description matches rig exactly
        let description = if LINE_NUMBERS {
            "Search file contents using regex patterns. Returns matches with file paths, line numbers, and content, sorted by file modification time."
        } else {
            "Search file contents using regex patterns. Returns matches with file paths and content, sorted by file modification time."
        };
        let schema = SchemaBuilder::new()
            .string(
                "pattern",
                "Regular expression pattern to search for in file contents",
                true,
            )
            .string("path", "Absolute directory path to search in", true)
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

        ToolDefinition::new("Grep", description).with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: GrepArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("Grep", None, e.to_string()))?;

        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Err(ToolError::validation_error(
                "Grep",
                Some("pattern".to_string()),
                "pattern must not be empty".to_string(),
            ));
        }

        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        if limit == 0 {
            return Err(ToolError::validation_error(
                "Grep",
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

        let resolver = AbsolutePathResolver;
        let result = grep_search(&resolver, pattern, include, &args.path, limit);

        match result {
            Err(e) => to_serdes_result("Grep", Err(e)),
            Ok(grep_output) => {
                if grep_output.files.is_empty() {
                    return Ok(ToolReturn::text("No matches found."));
                }

                // Format output grouped by file
                let mut output = String::with_capacity(4096);
                let _ = writeln!(&mut output, "Found {} matches", grep_output.match_count);

                for file in &grep_output.files {
                    let _ = writeln!(&mut output, "\n{}:", file.path);
                    for m in &file.matches {
                        // Truncate at UTF-8 boundary
                        let truncated_text = if m.line_text.len() > MAX_LINE_LENGTH {
                            &m.line_text[..m.line_text.floor_char_boundary(MAX_LINE_LENGTH)]
                        } else {
                            &m.line_text
                        };
                        if LINE_NUMBERS {
                            let _ = writeln!(&mut output, "  L{}: {}", m.line_num, truncated_text);
                        } else {
                            let _ = writeln!(&mut output, "  {}", truncated_text);
                        }
                    }
                }

                if grep_output.truncated {
                    let _ = write!(&mut output, "\n(Results truncated at {} matches)", limit);
                }

                Ok(ToolReturn::text(output))
            }
        }
    }
}

impl<const LINE_NUMBERS: bool> ToolContext for GrepTool<LINE_NUMBERS> {
    const NAME: &'static str = "Grep";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::GREP_ABSOLUTE
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
    async fn finds_content_with_required_path() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world\nfoo bar").unwrap();

        let tool: GrepTool<true> = GrepTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "hello",
                    "path": dir.path().to_string_lossy()
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("Found 1 matches"));
        assert!(text.contains("L1: hello world"));
    }

    #[tokio::test]
    async fn validates_limit() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();

        let tool: GrepTool<true> = GrepTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "hello",
                    "path": dir.path().to_string_lossy(),
                    "limit": 0
                }),
            )
            .await;

        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ValidationFailed { .. }));
        // Check the error contains the validation message
        match err {
            ToolError::ValidationFailed { errors, .. } => {
                assert!(!errors.is_empty());
                assert!(errors[0].message.contains("limit"));
            }
            _ => panic!("Expected ValidationFailed"),
        }
    }

    #[tokio::test]
    async fn returns_no_matches_message_when_empty() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let tool: GrepTool<true> = GrepTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "nonexistent_pattern_xyz",
                    "path": dir.path().to_string_lossy()
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert_eq!(text, "No matches found.");
    }

    #[tokio::test]
    async fn include_filter_restricts_to_matching_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn hello() {}").unwrap();
        std::fs::write(dir.path().join("code.py"), "def hello(): pass").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello world").unwrap();

        let tool: GrepTool<true> = GrepTool::new();

        // Search only .rs files
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "hello",
                    "path": dir.path().to_string_lossy(),
                    "include": "*.rs"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("Found 1 matches"));
        assert!(text.contains("code.rs"));
        assert!(!text.contains("code.py"));
        assert!(!text.contains("readme.txt"));
    }

    #[tokio::test]
    async fn truncates_long_lines_at_max_length() {
        let dir = TempDir::new().unwrap();
        // Create a line longer than MAX_LINE_LENGTH (2000 chars)
        let long_line = format!("prefix_{}_suffix", "x".repeat(2500));
        std::fs::write(dir.path().join("long.txt"), &long_line).unwrap();

        let tool: GrepTool<true> = GrepTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "prefix",
                    "path": dir.path().to_string_lossy()
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("Found 1 matches"));
        // The line should be truncated - it should contain prefix but not suffix
        assert!(text.contains("prefix_"));
        assert!(!text.contains("_suffix"));
        // Verify the match line doesn't exceed MAX_LINE_LENGTH
        for line in text.lines() {
            if line.contains("prefix_") {
                // Line format is "  L1: content", so actual content is line.len() - prefix
                let content_start = line.find("prefix_").unwrap();
                let content = &line[content_start..];
                assert!(content.len() <= MAX_LINE_LENGTH);
            }
        }
    }
}
