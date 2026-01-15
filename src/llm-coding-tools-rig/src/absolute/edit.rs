//! Edit file tool using [`AbsolutePathResolver`].

use llm_coding_tools_core::operations::edit_file;
use llm_coding_tools_core::path::AbsolutePathResolver;
pub use llm_coding_tools_core::EditError;
use llm_coding_tools_core::ToolContext;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;

/// Arguments for file editing.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditArgs {
    /// Absolute path to the file to modify.
    pub file_path: String,
    /// Exact text to find and replace.
    pub old_string: String,
    /// Replacement text.
    pub new_string: String,
    /// Replace all occurrences (default false).
    #[serde(default)]
    pub replace_all: bool,
}

/// Tool for making exact string replacements in files.
#[derive(Debug, Clone, Default)]
pub struct EditTool;

impl EditTool {
    /// Creates a new edit tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
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
            description: "Makes exact string replacements in files. Use replace_all=true to \
                           replace all occurrences."
                .to_string(),
            parameters: serde_json::to_value(schema_for!(EditArgs))
                .expect("EditArgs schema generation should not fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let resolver = AbsolutePathResolver;
        edit_file(
            &resolver,
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
        llm_coding_tools_core::context::EDIT_ABSOLUTE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_core::ToolError;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn replaces_single_occurrence() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();
        let tool = EditTool::new();
        let result = tool
            .call(EditArgs {
                file_path: file.path().to_string_lossy().to_string(),
                old_string: "world".to_string(),
                new_string: "rust".to_string(),
                replace_all: false,
            })
            .await
            .unwrap();
        assert!(result.contains("1 occurrence"));
    }

    #[tokio::test]
    async fn rejects_relative_path() {
        let tool = EditTool::new();
        let result = tool
            .call(EditArgs {
                file_path: "relative/path.txt".to_string(),
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
