//! Edit file tool using [`AllowedPathResolver`].

use llm_coding_tools_core::operations::edit_file;
use llm_coding_tools_core::path::AllowedPathResolver;
pub use llm_coding_tools_core::EditError;
use llm_coding_tools_core::{ToolContext, ToolResult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Arguments for file editing.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditArgs {
    /// Relative path to the file to modify (within allowed directories).
    pub file_path: String,
    /// Exact text to find and replace.
    pub old_string: String,
    /// Replacement text.
    pub new_string: String,
    /// Replace all occurrences (default false).
    #[serde(default)]
    pub replace_all: bool,
}

/// Tool for making exact string replacements in files within allowed directories.
#[derive(Debug, Clone)]
pub struct EditTool {
    resolver: AllowedPathResolver,
}

impl EditTool {
    /// Creates a new edit tool restricted to the given directories.
    pub fn new(allowed_paths: impl IntoIterator<Item = impl AsRef<Path>>) -> ToolResult<Self> {
        let paths: Vec<PathBuf> = allowed_paths
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        Ok(Self {
            resolver: AllowedPathResolver::new(paths)?,
        })
    }

    /// Creates a new edit tool with an existing resolver.
    pub fn with_resolver(resolver: AllowedPathResolver) -> Self {
        Self { resolver }
    }
}

impl Tool for EditTool {
    const NAME: &'static str = "edit";

    type Error = EditError;
    type Args = EditArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: "Make exact string replacements in files within allowed directories. \
                          Paths are relative to configured base directories."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(EditArgs))
                .expect("EditArgs schema generation should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        edit_file(
            &self.resolver,
            &args.file_path,
            &args.old_string,
            &args.new_string,
            args.replace_all,
        )
        .await
    }
}

impl ToolContext for EditTool {
    const NAME: &'static str = "edit";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::EDIT_ALLOWED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::ToolError;
    use tempfile::TempDir;

    #[tokio::test]
    async fn replaces_single_occurrence() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let tool = EditTool::new([dir.path()]).unwrap();
        let result = tool
            .call(EditArgs {
                file_path: "test.txt".to_string(),
                old_string: "world".to_string(),
                new_string: "rust".to_string(),
                replace_all: false,
            })
            .await
            .unwrap();
        assert!(result.contains("1 occurrence"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let tool = EditTool::new([dir.path()]).unwrap();
        let result = tool
            .call(EditArgs {
                file_path: "../../../etc/passwd".to_string(),
                old_string: "old".to_string(),
                new_string: "new".to_string(),
                replace_all: false,
            })
            .await;
        assert!(matches!(
            result,
            Err(EditError::Tool(ToolError::InvalidPath(_)))
        ));
    }
}
