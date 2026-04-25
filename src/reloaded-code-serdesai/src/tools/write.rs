//! File writing tool using any [`PathResolver`].
//!
//! Writes string content to a file, creating the file if it does not exist
//! or overwriting it if it does.
//!
//! # Public API
//!
//! - [`WriteTool`] - adapter implementing [`Tool`] for file writing
//!
//! [`Tool`]: serdes_ai::tools::Tool

use async_trait::async_trait;
use reloaded_code_core::context::{PathMode, ToolPrompt};
use reloaded_code_core::path::PathResolver;
use reloaded_code_core::tool_metadata::write as write_meta;
use reloaded_code_core::tools::{WriteRequest, WriteSettings, write_file};
use reloaded_code_core::{ToolContext, ToolOutput};
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolResult};

use crate::convert::{core_error_to_serdes, to_serdes_result};

/// Tool for writing content to files.
///
/// Generic over any [`PathResolver`] implementation.
#[derive(Debug, Clone)]
pub struct WriteTool<R: PathResolver + Clone> {
    definition: ToolDefinition,
    resolver: R,
    path_mode: PathMode,
    settings: WriteSettings,
}

impl<R: PathResolver + Clone> WriteTool<R> {
    /// Creates a new write tool with the given resolver.
    ///
    /// # Type Parameters
    ///
    /// * `R` - A path resolver implementing [`PathResolver`].
    pub fn new(resolver: R) -> Self {
        Self::with_settings(resolver, WriteSettings::new())
    }

    /// Creates a new write tool with custom settings.
    ///
    /// # Arguments
    ///
    /// * `resolver` - A [`PathResolver`] used to resolve and validate file paths.
    /// * `settings` - [`WriteSettings`] controlling overwrite handling.
    pub fn with_settings(resolver: R, settings: WriteSettings) -> Self {
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
impl<R: PathResolver + Clone + Send + Sync, Deps: Send + Sync> Tool<Deps> for WriteTool<R> {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args =
            WriteRequest::parse(args).map_err(|e| core_error_to_serdes(write_meta::NAME, e))?;

        let result = write_file(&self.resolver, args, &self.settings).await;
        to_serdes_result(write_meta::NAME, result.map(ToolOutput::new))
    }
}

impl<R: PathResolver + Clone> ToolContext for WriteTool<R> {
    const NAME: &'static str = write_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Write {
            path_mode: self.path_mode,
        }
    }
}

fn build_definition(path_mode: PathMode) -> ToolDefinition {
    let (file_path_param, description) = match path_mode {
        PathMode::Absolute => (
            write_meta::param::FILE_PATH_ABSOLUTE,
            write_meta::description::ABSOLUTE,
        ),
        PathMode::Allowed => (
            write_meta::param::FILE_PATH_ALLOWED,
            write_meta::description::ALLOWED,
        ),
    };

    let schema = SchemaBuilder::new()
        .string(
            file_path_param.name,
            file_path_param.description,
            file_path_param.required,
        )
        .string(
            write_meta::param::CONTENT.name,
            write_meta::param::CONTENT.description,
            write_meta::param::CONTENT.required,
        )
        .build()
        .expect("schema build should not fail");

    ToolDefinition {
        name: write_meta::NAME.to_owned(),
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
    use serdes_ai::tools::RunContext;
    use tempfile::TempDir;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn writes_file_with_absolute_resolver() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("new.txt");
        let tool = WriteTool::new(AbsolutePathResolver);

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": file_path.to_string_lossy(),
                    "content": "hello world"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("11 bytes"));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
    }

    #[tokio::test]
    async fn writes_new_file_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = WriteTool::new(resolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "new.txt",
                    "content": "hello"
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("5 bytes"));
        assert!(dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn rejects_path_traversal_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = WriteTool::new(resolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "../../../tmp/escape.txt",
                    "content": "content"
                }),
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn determines_correct_path_mode_for_absolute_resolver() {
        let tool = WriteTool::new(AbsolutePathResolver);
        assert_eq!(tool.path_mode(), PathMode::Absolute);
    }

    #[test]
    fn determines_correct_path_mode_for_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = WriteTool::new(resolver);
        assert_eq!(tool.path_mode(), PathMode::Allowed);
    }
}
