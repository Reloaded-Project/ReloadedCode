//! Glob pattern file finding tool using [`AllowedPathResolver`].

use llm_coding_tools_core::operations::glob_files;
use llm_coding_tools_core::path::AllowedPathResolver;
use llm_coding_tools_core::{GlobOutput, ToolContext, ToolError, ToolResult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::path::Path;

/// Arguments for the glob tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GlobArgs {
    /// Glob pattern to match files against (e.g., "**/*.rs", "src/**/*.ts").
    pub pattern: String,
    /// Relative directory path to search in (within allowed directories).
    pub path: String,
}

/// Tool for finding files matching glob patterns within allowed directories.
#[derive(Debug, Clone)]
pub struct GlobTool {
    resolver: AllowedPathResolver,
}

impl GlobTool {
    /// Creates a new glob tool restricted to the given directories.
    pub fn new(allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> ToolResult<Self> {
        Ok(Self {
            resolver: AllowedPathResolver::new(allowed_paths)?,
        })
    }

    /// Creates a new glob tool with an existing resolver.
    pub fn with_resolver(resolver: AllowedPathResolver) -> Self {
        Self { resolver }
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
            description: "Find files matching a glob pattern within allowed directories. \
                          Paths are relative to configured base directories."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(GlobArgs))
                .expect("schema serialization should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        glob_files(&self.resolver, &args.pattern, &args.path)
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
    use std::fs::{self, File};
    use tempfile::TempDir;

    #[tokio::test]
    async fn finds_matching_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        File::create(dir.path().join("src/lib.rs")).unwrap();

        let tool = GlobTool::new([dir.path()]).unwrap();
        let result = tool
            .call(GlobArgs {
                pattern: "**/*.rs".to_string(),
                path: ".".to_string(),
            })
            .await
            .unwrap();
        assert!(result.files.iter().any(|f| f.ends_with("lib.rs")));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool = GlobTool::new([dir.path()]).unwrap();
        let result = tool
            .call(GlobArgs {
                pattern: "*.rs".to_string(),
                path: "../../../etc".to_string(),
            })
            .await;
        assert!(matches!(result, Err(ToolError::InvalidPath(_))));
    }
}
