//! Regex-based content search across files, using any [`PathResolver`].
//!
//! Searches file contents using regular expression patterns with optional
//! file-type filtering. Matching lines are returned with line numbers and
//! truncated at a configurable maximum length.
//!
//! # Public API
//!
//! - [`GrepTool`] - adapter implementing [`Tool`] for content search
//!
//! [`Tool`]: serdes_ai::tools::Tool

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::context::{PathMode, ToolPrompt};
use llm_coding_tools_core::path::PathResolver;
use llm_coding_tools_core::tool_metadata::grep as grep_meta;
use llm_coding_tools_core::tools::{DEFAULT_MAX_LINE_LENGTH, GrepOutput, grep_search};
use serde::Deserialize;
use serde_json::json;
use serdes_ai::tools::{
    RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult, ToolReturn,
};

use crate::convert::to_serdes_result;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct GrepArgs {
    pattern: String,
    path: String,
    #[serde(default)]
    include: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

/// Tool for searching file contents using regex patterns.
///
/// Generic over any [`PathResolver`] implementation.
#[derive(Debug, Clone)]
pub struct GrepTool<R: PathResolver + Clone> {
    definition: ToolDefinition,
    resolver: R,
    path_mode: PathMode,
    max_line_length: usize,
    limit: usize,
    line_numbers: bool,
}

impl<R: PathResolver + Clone> GrepTool<R> {
    /// Creates a new grep tool with the given resolver and default settings.
    ///
    /// Uses `max_line_length` of 2000 characters, `limit` of 100 matches,
    /// and enables line numbers.
    ///
    /// # Type Parameters
    ///
    /// * `R` - A path resolver implementing [`PathResolver`].
    pub fn new(resolver: R) -> Self {
        Self::with_settings(
            resolver,
            DEFAULT_MAX_LINE_LENGTH,
            grep_meta::DEFAULT_LIMIT,
            true,
        )
    }

    /// Creates a new grep tool with custom settings.
    ///
    /// # Arguments
    ///
    /// * `resolver` - The path resolver for path validation.
    /// * `max_line_length` - Maximum characters per matching line before truncation.
    /// * `limit` - Maximum number of matches to return when not specified in args.
    /// * `line_numbers` - Whether to prefix lines with line numbers.
    pub fn with_settings(
        resolver: R,
        max_line_length: usize,
        limit: usize,
        line_numbers: bool,
    ) -> Self {
        let path_mode = R::PATH_MODE;
        Self {
            definition: build_definition(path_mode, line_numbers),
            resolver,
            path_mode,
            max_line_length,
            limit,
            line_numbers,
        }
    }

    /// Returns the path mode for this tool instance.
    #[must_use]
    pub fn path_mode(&self) -> PathMode {
        self.path_mode
    }
}

#[async_trait]
impl<R: PathResolver + Clone + Send + Sync, Deps: Send + Sync> Tool<Deps> for GrepTool<R> {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: GrepArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(grep_meta::NAME, None, e.to_string()))?;

        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Err(ToolError::validation_error(
                grep_meta::NAME,
                Some("pattern".to_string()),
                "pattern must not be empty".to_string(),
            ));
        }

        let limit = args.limit.unwrap_or(self.limit);
        if limit == 0 {
            return Err(ToolError::validation_error(
                grep_meta::NAME,
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
            Err(e) => to_serdes_result(grep_meta::NAME, Err(e)),
            Ok(grep_output) => Ok(grep_output_to_return(
                grep_output,
                self.line_numbers,
                limit,
                self.max_line_length,
            )),
        }
    }
}

const NO_MATCHES_FOUND: &str = "No matches found.";

fn grep_output_to_return(
    output: GrepOutput,
    line_numbers: bool,
    limit: usize,
    max_line_len: usize,
) -> ToolReturn {
    if output.partial {
        let content = output.format(line_numbers, limit, max_line_len);
        return ToolReturn::json(json!({
            "content": content,
            "partial": true,
            "errors": output.errors,
            "match_count": output.match_count,
            "truncated": output.truncated,
        }));
    }

    if output.files.is_empty() {
        return ToolReturn::text(NO_MATCHES_FOUND);
    }

    ToolReturn::text(output.format(line_numbers, limit, max_line_len))
}

impl<R: PathResolver + Clone> ToolContext for GrepTool<R> {
    const NAME: &'static str = grep_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Grep {
            path_mode: self.path_mode,
            line_numbers: self.line_numbers,
        }
    }
}

fn build_definition(path_mode: PathMode, line_numbers: bool) -> ToolDefinition {
    let (path_param, description) = match path_mode {
        PathMode::Absolute => (
            grep_meta::param::PATH_ABSOLUTE,
            grep_meta::description::absolute(line_numbers),
        ),
        PathMode::Allowed => (
            grep_meta::param::PATH_ALLOWED,
            grep_meta::description::allowed(line_numbers),
        ),
    };

    let schema = SchemaBuilder::new()
        .string(
            grep_meta::param::PATTERN.name,
            grep_meta::param::PATTERN.description,
            grep_meta::param::PATTERN.required,
        )
        .string(path_param.name, path_param.description, path_param.required)
        .string(
            grep_meta::param::INCLUDE.name,
            grep_meta::param::INCLUDE.description,
            grep_meta::param::INCLUDE.required,
        )
        .integer_constrained(
            grep_meta::param::LIMIT.name,
            grep_meta::param::LIMIT.description,
            grep_meta::param::LIMIT.required,
            Some(1),
            None,
        )
        .build()
        .expect("schema build should not fail");

    ToolDefinition {
        name: grep_meta::NAME.to_owned(),
        description: description.to_owned(),
        parameters_json_schema: schema,
        strict: None,
        outer_typed_dict_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::path::AbsolutePathResolver;
    use llm_coding_tools_core::path::AllowedPathResolver;
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use tempfile::TempDir;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn finds_content_with_absolute_resolver() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world\nfoo bar").unwrap();

        let tool = GrepTool::new(AbsolutePathResolver);
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
    async fn finds_matching_content_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GrepTool::new(resolver);
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
    async fn rejects_path_traversal_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GrepTool::new(resolver);
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
    async fn validates_limit() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();

        let tool = GrepTool::new(AbsolutePathResolver);
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
    }

    #[tokio::test]
    async fn returns_no_matches_message_when_empty() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let tool = GrepTool::new(AbsolutePathResolver);
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

    // PORTED TESTS from absolute/grep.rs and allowed/grep.rs

    #[tokio::test]
    async fn include_filter_restricts_to_matching_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn hello() {}").unwrap();
        std::fs::write(dir.path().join("code.py"), "def hello(): pass").unwrap();
        std::fs::write(dir.path().join("readme.txt"), "hello world").unwrap();

        let tool = GrepTool::new(AbsolutePathResolver);

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

        let tool = GrepTool::new(AbsolutePathResolver);
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
        // Verify the match line doesn't exceed DEFAULT_MAX_LINE_LENGTH
        for line in text.lines() {
            if line.contains("prefix_") {
                // Line format is "  L1: content", so actual content is line.len() - prefix
                let content_start = line.find("prefix_").unwrap();
                let content = &line[content_start..];
                assert!(content.len() <= DEFAULT_MAX_LINE_LENGTH);
            }
        }
    }

    #[tokio::test]
    async fn returns_partial_json_when_search_has_errors() {
        let dir = TempDir::new().unwrap();
        let missing_path = dir.path().join("missing-root");
        let tool = GrepTool::new(AbsolutePathResolver);

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "hello",
                    "path": missing_path.to_string_lossy()
                }),
            )
            .await
            .unwrap();

        let payload = result.as_json().unwrap();
        assert_eq!(payload["partial"], true);
        assert!(!payload["errors"].as_array().unwrap().is_empty());
        assert!(
            payload["content"]
                .as_str()
                .unwrap()
                .contains("Partial results")
        );
    }

    #[tokio::test]
    async fn rejects_empty_pattern() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GrepTool::new(resolver);
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
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ValidationFailed { .. }));
    }

    #[test]
    fn determines_correct_path_mode_for_absolute_resolver() {
        let tool = GrepTool::new(AbsolutePathResolver);
        assert_eq!(tool.path_mode(), PathMode::Absolute);
    }

    #[test]
    fn determines_correct_path_mode_for_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GrepTool::new(resolver);
        assert_eq!(tool.path_mode(), PathMode::Allowed);
    }
}
