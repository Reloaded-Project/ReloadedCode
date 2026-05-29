//! Exact string replacement tool for files, using any [`PathResolver`].
//!
//! Performs find-and-replace operations on file contents. Supports replacing
//! a single occurrence or all occurrences of a given string.
//!
//! # Public API
//!
//! - [`EditTool`] - adapter implementing [`Tool`] for file editing
//!
//! [`Tool`]: serdes_ai::tools::Tool

use async_trait::async_trait;
use reloaded_code_core::ToolContext;
use reloaded_code_core::context::{PathMode, ToolPrompt};
use reloaded_code_core::path::PathResolver;
use reloaded_code_core::tool_metadata::edit as edit_meta;
use reloaded_code_core::tools::{EditRequest, EditSettings, edit_file};
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolResult, ToolReturn};

use crate::convert::core_error_to_serdes;

/// Tool for making exact string replacements in files.
///
/// Generic over any [`PathResolver`] implementation.
#[derive(Debug, Clone)]
pub struct EditTool<R: PathResolver + Clone> {
    definition: ToolDefinition,
    resolver: R,
    path_mode: PathMode,
    settings: EditSettings,
}

impl<R: PathResolver + Clone> EditTool<R> {
    /// Creates a new edit tool with the given resolver.
    ///
    /// # Type Parameters
    ///
    /// * `R` - A path resolver implementing [`PathResolver`].
    pub fn new(resolver: R) -> Self {
        Self::with_settings(resolver, EditSettings::new())
    }

    /// Creates a new edit tool with custom settings.
    ///
    /// # Arguments
    ///
    /// * `resolver` - A [`PathResolver`] used to resolve and validate file paths.
    /// * `settings` - [`EditSettings`] controlling replacement limits.
    pub fn with_settings(resolver: R, settings: EditSettings) -> Self {
        let path_mode = resolver.path_mode();
        Self {
            definition: build_definition(path_mode),
            resolver,
            path_mode,
            settings,
        }
    }

    /// Returns the path mode for this tool instance.
    #[must_use]
    pub fn path_mode(&self) -> PathMode {
        self.path_mode
    }
}

#[async_trait]
impl<R: PathResolver + Clone + Send + Sync, Deps: Send + Sync> Tool<Deps> for EditTool<R> {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args =
            EditRequest::parse(args).map_err(|e| core_error_to_serdes(edit_meta::NAME, e))?;
        let result = edit_file(&self.resolver, args, &self.settings).await;

        result
            .map(ToolReturn::text)
            .map_err(|e| core_error_to_serdes(edit_meta::NAME, e.into()))
    }
}

impl<R: PathResolver + Clone> ToolContext for EditTool<R> {
    fn name(&self) -> &'static str {
        edit_meta::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Edit {
            path_mode: self.path_mode,
        }
    }
}

fn build_definition(path_mode: PathMode) -> ToolDefinition {
    let (file_path_param, description) = match path_mode {
        PathMode::Absolute => (
            edit_meta::param::FILE_PATH_ABSOLUTE,
            edit_meta::description::ABSOLUTE,
        ),
        PathMode::Allowed => (
            edit_meta::param::FILE_PATH_ALLOWED,
            edit_meta::description::ALLOWED,
        ),
    };

    let schema = SchemaBuilder::new()
        .string(
            file_path_param.name,
            file_path_param.description,
            file_path_param.required,
        )
        .string(
            edit_meta::param::OLD_STRING.name,
            edit_meta::param::OLD_STRING.description,
            edit_meta::param::OLD_STRING.required,
        )
        .string(
            edit_meta::param::NEW_STRING.name,
            edit_meta::param::NEW_STRING.description,
            edit_meta::param::NEW_STRING.required,
        )
        .boolean(
            edit_meta::param::REPLACE_ALL.name,
            edit_meta::param::REPLACE_ALL.description,
            edit_meta::param::REPLACE_ALL.required,
        )
        .build()
        .expect("schema build should not fail");

    ToolDefinition {
        name: edit_meta::NAME.to_owned(),
        description: description.to_owned(),
        parameters_json_schema: schema,
        strict: None,
        outer_typed_dict_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reloaded_code_core::path::AbsolutePathResolver;
    use reloaded_code_core::path::AllowedPathResolver;
    use serde_json::json;
    use serdes_ai::tools::{RunContext, ToolError};
    use std::io::Write as _;
    use tempfile::{NamedTempFile, TempDir};

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn edit_success_with_absolute_resolver() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let tool = EditTool::new(AbsolutePathResolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file.path().to_string_lossy(),
                    "old_string": "world",
                    "new_string": "rust"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("1 occurrence"));
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "hello rust");
    }

    #[tokio::test]
    async fn replaces_single_occurrence_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = EditTool::new(resolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "test.txt",
                    "old_string": "world",
                    "new_string": "rust"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("1 occurrence"));
    }

    #[tokio::test]
    async fn rejects_path_traversal_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = EditTool::new(resolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "../../../etc/passwd",
                    "old_string": "old",
                    "new_string": "new"
                }),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn edit_not_found_error() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();

        let tool = EditTool::new(AbsolutePathResolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file.path().to_string_lossy(),
                    "old_string": "not_found",
                    "new_string": "replacement"
                }),
            )
            .await;

        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ValidationFailed { .. }));
    }

    #[tokio::test]
    async fn edit_ambiguous_match_error() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello hello hello").unwrap();
        file.flush().unwrap();

        let tool = EditTool::new(AbsolutePathResolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file.path().to_string_lossy(),
                    "old_string": "hello",
                    "new_string": "world",
                    "replace_all": false
                }),
            )
            .await;

        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ValidationFailed { .. }));
        // Check the error contains the validation message
        match err {
            ToolError::ValidationFailed { errors, .. } => {
                assert!(!errors.is_empty());
                assert!(errors[0].message.contains("multiple times"));
            }
            _ => panic!("Expected ValidationFailed"),
        }
    }

    #[test]
    fn determines_correct_path_mode_for_absolute_resolver() {
        let tool = EditTool::new(AbsolutePathResolver);
        assert_eq!(tool.path_mode(), PathMode::Absolute);
    }

    #[test]
    fn determines_correct_path_mode_for_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = EditTool::new(resolver);
        assert_eq!(tool.path_mode(), PathMode::Allowed);
    }
}
