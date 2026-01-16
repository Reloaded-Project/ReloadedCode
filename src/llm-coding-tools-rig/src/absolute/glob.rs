//! Glob pattern file finding tool using [`AbsolutePathResolver`].

use llm_coding_tools_core::operations::glob_files;
use llm_coding_tools_core::path::AbsolutePathResolver;
use llm_coding_tools_core::{GlobOutput, ToolContext, ToolError};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

/// Arguments for the glob tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GlobArgs {
    /// Glob pattern to match files against (e.g., "**/*.rs", "src/**/*.ts").
    pub pattern: String,
    /// Absolute directory path to search in.
    pub path: String,
}

/// Tool for finding files matching glob patterns.
#[derive(Debug, Default, Clone, Copy)]
pub struct GlobTool;

impl GlobTool {
    /// Creates a new glob tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Tool for GlobTool {
    const NAME: &'static str = "Glob";

    type Error = ToolError;
    type Args = GlobArgs;
    type Output = GlobOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Find files matching a glob pattern. Respects .gitignore and \
                returns paths sorted by modification time (newest first)."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(GlobArgs))
                .expect("schema serialization should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resolver = AbsolutePathResolver;
        glob_files(&resolver, &args.pattern, &args.path)
    }
}

impl ToolContext for GlobTool {
    const NAME: &'static str = "Glob";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::GLOB_ABSOLUTE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    #[tokio::test]
    async fn finds_matching_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        File::create(dir.path().join("src/lib.rs")).unwrap();
        let tool = GlobTool::new();
        let result = tool
            .call(GlobArgs {
                pattern: "**/*.rs".to_string(),
                path: dir.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
        assert!(result.files.iter().any(|f| f.ends_with("lib.rs")));
    }

    #[tokio::test]
    async fn rejects_relative_path() {
        let tool = GlobTool::new();
        let result = tool
            .call(GlobArgs {
                pattern: "*.rs".to_string(),
                path: "relative/path".to_string(),
            })
            .await;
        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }
}
