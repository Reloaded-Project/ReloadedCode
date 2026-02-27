//! Glob pattern file finding tool using [`AbsolutePathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::ToolContext;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::tools::glob_files;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};

use crate::common::glob::output_to_return as glob_output_to_return;
use crate::convert::to_serdes_result;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct GlobArgs {
    /// Glob pattern to match files (e.g., "**/*.rs", "src/**/*.ts").
    pattern: String,
    /// Absolute directory path to search in.
    path: String,
}

/// Tool for finding files matching glob patterns.
///
/// Respects `.gitignore` and returns paths sorted by modification time (newest first).
#[derive(Debug, Clone, Default)]
pub struct GlobTool;

impl GlobTool {
    /// Creates a new glob tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for GlobTool {
    fn definition(&self) -> ToolDefinition {
        let schema = SchemaBuilder::new()
            .string(
                "pattern",
                "Glob pattern to match files (e.g., \"**/*.rs\", \"src/**/*.ts\")",
                true,
            )
            .string("path", "Absolute directory path to search in", true)
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new(
             tool_names::GLOB,
             "Find files matching a glob pattern. Respects .gitignore and returns paths sorted by modification time (newest first).",
         )
         .with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: GlobArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::GLOB, None, e.to_string()))?;

        let resolver = AbsolutePathResolver;
        let result = glob_files(&resolver, &args.pattern, &args.path);

        match result {
            Err(e) => to_serdes_result(tool_names::GLOB, Err(e)),
            Ok(output) => Ok(glob_output_to_return(output)),
        }
    }
}

impl ToolContext for GlobTool {
    const NAME: &'static str = tool_names::GLOB;

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::GLOB_ABSOLUTE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::tools::GlobOutput;
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn finds_files_with_required_path() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        File::create(dir.path().join("src/lib.rs")).unwrap();
        File::create(dir.path().join("src/main.rs")).unwrap();

        let tool = GlobTool::new();
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "**/*.rs",
                    "path": dir.path().to_string_lossy()
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("lib.rs"));
        assert!(text.contains("main.rs"));
    }

    #[test]
    fn partial_glob_output_returns_json_payload() {
        let payload = glob_output_to_return(GlobOutput {
            files: vec!["src/lib.rs".to_string()],
            truncated: false,
            partial: true,
            errors: vec!["walk error: denied".to_string()],
        });

        let json = payload.as_json().unwrap();
        assert_eq!(json["partial"], true);
        assert_eq!(json["errors"][0], "walk error: denied");
    }
}
