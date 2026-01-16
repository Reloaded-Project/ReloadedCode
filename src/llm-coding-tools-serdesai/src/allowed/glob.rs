//! Glob pattern file finding tool using [`AllowedPathResolver`].

use async_trait::async_trait;
use llm_coding_tools_core::operations::glob_files;
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::{ToolContext, ToolOutput};
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::path::PathBuf;

use crate::convert::to_serdes_result;

/// Internal args for JSON deserialization.
#[derive(Debug, Deserialize)]
struct GlobArgs {
    /// Glob pattern to match files (e.g., "**/*.rs", "src/**/*.ts").
    pattern: String,
    /// Directory path to search in (relative to allowed directories).
    path: String,
}

/// Tool for finding files matching glob patterns within allowed directories.
#[derive(Debug, Clone)]
pub struct GlobTool {
    resolver: AllowedPathResolver,
}

impl GlobTool {
    /// Creates a new glob tool restricted to the given directories.
    pub fn new(allowed_directories: Vec<PathBuf>) -> Self {
        Self {
            resolver: AllowedPathResolver::from_canonical(allowed_directories),
        }
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
            .string(
                "path",
                "Directory path to search in (relative to allowed directories)",
                true,
            )
            .build()
            .expect("schema build should not fail");

        ToolDefinition::new(
            "Glob",
            "Find files matching a glob pattern within allowed directories. \
             Paths are relative to configured base directories.",
        )
        .with_parameters(schema)
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: GlobArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("Glob", None, e.to_string()))?;

        let result = glob_files(&self.resolver, &args.pattern, &args.path);
        to_serdes_result(
            "Glob",
            result.map(|output| {
                let content = if output.files.is_empty() {
                    "No files found matching the pattern.".to_string()
                } else {
                    output.files.join("\n")
                };
                if output.truncated {
                    ToolOutput::truncated(content)
                } else {
                    ToolOutput::new(content)
                }
            }),
        )
    }
}

impl ToolContext for GlobTool {
    const NAME: &'static str = "Glob";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::GLOB_ALLOWED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn finds_matching_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        File::create(dir.path().join("src/lib.rs")).unwrap();

        let tool = GlobTool::new(vec![dir.path().to_path_buf()]);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "**/*.rs",
                    "path": "."
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("lib.rs"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool = GlobTool::new(vec![dir.path().to_path_buf()]);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "pattern": "*.rs",
                    "path": "../../../etc"
                }),
            )
            .await;

        assert!(result.is_err());
    }
}
