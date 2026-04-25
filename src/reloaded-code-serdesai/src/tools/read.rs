//! File reading tool using any [`PathResolver`].
//!
//! Reads file contents with optional offset and line limit. Lines can be
//! prefixed with line numbers and are truncated at a configurable maximum
//! length.
//!
//! Works with [`AbsolutePathResolver`], [`AllowedPathResolver`], or custom resolvers.
//!
//! # Public API
//!
//! - [`ReadTool`] - adapter implementing [`Tool`] for file reading
//!
//! # Example
//!
//! ```no_run
//! use reloaded_code_serdesai::{ReadTool, AbsolutePathResolver};
//!
//! let tool = ReadTool::new(AbsolutePathResolver);
//! ```
//!
//! [`PathResolver`]: reloaded_code_core::path::PathResolver
//! [`AbsolutePathResolver`]: reloaded_code_core::path::AbsolutePathResolver
//! [`AllowedPathResolver`]: reloaded_code_core::path::AllowedPathResolver
//! [`Tool`]: serdes_ai::tools::Tool

use async_trait::async_trait;
use reloaded_code_core::ToolContext;
use reloaded_code_core::context::{PathMode, ToolPrompt};
use reloaded_code_core::path::PathResolver;
use reloaded_code_core::tool_metadata::read as read_meta;
use reloaded_code_core::tools::{ReadRequest, ReadSettings, read_file};
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolResult};

use crate::convert::{core_error_to_serdes, to_serdes_result};

/// Tool for reading file contents with optional line ranges and numbers.
///
/// Generic over any [`PathResolver`] implementation. See the [module-level
/// documentation](crate::tools) for a usage example.
#[derive(Debug, Clone)]
pub struct ReadTool<R: PathResolver + Clone> {
    definition: ToolDefinition,
    resolver: R,
    path_mode: PathMode,
    settings: ReadSettings,
}

impl<R: PathResolver + Clone> ReadTool<R> {
    /// Creates a new read tool with the given resolver and default settings.
    ///
    /// Uses `limit` of 2000 lines, `max_line_length` of 2000 characters,
    /// and enables line numbers.
    ///
    /// # Type Parameters
    ///
    /// * `R` - A path resolver implementing [`PathResolver`].
    pub fn new(resolver: R) -> Self {
        Self::with_settings(resolver, ReadSettings::new())
    }

    /// Creates a new read tool with custom settings.
    ///
    /// # Arguments
    ///
    /// * `resolver` - The path resolver for path validation and resolution.
    /// * `settings` - Core read settings for limits and formatting.
    pub fn with_settings(resolver: R, settings: ReadSettings) -> Self {
        let path_mode = resolver.path_mode();
        Self {
            definition: build_definition(path_mode, settings.line_numbers()),
            resolver,
            path_mode,
            settings,
        }
    }

    /// Returns the path mode for this tool instance.
    ///
    /// The path mode comes from the resolver implementation.
    #[must_use]
    pub fn path_mode(&self) -> PathMode {
        self.path_mode
    }
}

#[async_trait]
impl<R: PathResolver + Clone + Send + Sync, Deps: Send + Sync> Tool<Deps> for ReadTool<R> {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args =
            ReadRequest::parse(args).map_err(|e| core_error_to_serdes(read_meta::NAME, e))?;

        let result = read_file(&self.resolver, args, &self.settings).await;
        to_serdes_result(read_meta::NAME, result)
    }
}

impl<R: PathResolver + Clone> ToolContext for ReadTool<R> {
    const NAME: &'static str = read_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Read {
            path_mode: self.path_mode,
            line_numbers: self.settings.line_numbers(),
        }
    }
}

fn build_definition(path_mode: PathMode, line_numbers: bool) -> ToolDefinition {
    let (file_path_param, description) = match path_mode {
        PathMode::Absolute => (
            read_meta::param::FILE_PATH_ABSOLUTE,
            read_meta::description::absolute(line_numbers),
        ),
        PathMode::Allowed => (
            read_meta::param::FILE_PATH_ALLOWED,
            read_meta::description::allowed(line_numbers),
        ),
    };

    let schema = SchemaBuilder::new()
        .string(
            file_path_param.name,
            file_path_param.description,
            file_path_param.required,
        )
        .integer_constrained(
            read_meta::param::OFFSET.name,
            read_meta::param::OFFSET.description,
            read_meta::param::OFFSET.required,
            Some(1),
            None,
        )
        .integer_constrained(
            read_meta::param::LIMIT.name,
            read_meta::param::LIMIT.description,
            read_meta::param::LIMIT.required,
            Some(1),
            None,
        )
        .build()
        .expect("schema build should not fail");

    ToolDefinition {
        name: read_meta::NAME.to_owned(),
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
    use serdes_ai::tools::{RunContext, Tool, ToolDefinition};
    use std::io::Write as _;
    use tempfile::{NamedTempFile, TempDir};

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[test]
    fn read_tool_should_use_custom_core_settings() {
        let settings = ReadSettings::new()
            .with_limits(1, 1)
            .unwrap()
            .with_max_line_length(100)
            .unwrap()
            .with_line_numbers(false);
        let tool = ReadTool::with_settings(AbsolutePathResolver, settings.clone());
        assert_eq!(tool.settings, settings);
    }

    #[tokio::test]
    async fn reads_file_with_absolute_resolver() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"line1\nline2\nline3\nline4\n").unwrap();
        let tool = ReadTool::new(AbsolutePathResolver);

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": temp.path().to_string_lossy(),
                    "offset": 2,
                    "limit": 2
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("2: line2"));
        assert!(text.contains("3: line3"));
        assert!(!text.contains("1:"));
        assert!(!text.contains("4:"));
    }

    #[tokio::test]
    async fn reads_file_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello\nworld\n").unwrap();

        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = ReadTool::new(resolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "test.txt",
                    "offset": 1,
                    "limit": 2000
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("1: hello"));
        assert!(text.contains("2: world"));
    }

    #[tokio::test]
    async fn rejects_path_traversal_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool: ReadTool<AllowedPathResolver> = ReadTool::new(resolver);
        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": "../../../etc/passwd"
                }),
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn respects_custom_settings() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"line1\nline2\n").unwrap();
        let settings = ReadSettings::new()
            .with_limits(1, 1)
            .unwrap()
            .with_max_line_length(100)
            .unwrap()
            .with_line_numbers(false);
        let tool = ReadTool::with_settings(AbsolutePathResolver, settings);

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": temp.path().to_string_lossy()
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("line1"));
        assert!(!text.contains("1:")); // line numbers disabled
    }

    #[tokio::test]
    async fn caps_requested_limit_at_tool_limit() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"line1\nline2\nline3\n").unwrap();
        let settings = ReadSettings::new()
            .with_limits(1, 1)
            .unwrap()
            .with_line_numbers(true);
        let tool = ReadTool::with_settings(AbsolutePathResolver, settings);

        let result = tool
            .call(
                &mock_ctx(),
                json!({
                    "file_path": temp.path().to_string_lossy(),
                    "limit": 3
                }),
            )
            .await
            .unwrap();

        let text = result.as_text().unwrap();
        assert!(text.contains("1: line1"));
        assert!(!text.contains("2: line2"));
        assert!(!text.contains("3: line3"));
    }

    #[test]
    fn determines_correct_path_mode_for_absolute_resolver() {
        let tool = ReadTool::new(AbsolutePathResolver);
        assert_eq!(tool.path_mode(), PathMode::Absolute);

        // Verify context returns correct path mode
        match tool.context() {
            ToolPrompt::Read { path_mode, .. } => {
                assert_eq!(path_mode, PathMode::Absolute);
            }
            _ => panic!("Expected ToolPrompt::Read"),
        }
    }

    #[test]
    fn determines_correct_path_mode_for_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = ReadTool::new(resolver);
        assert_eq!(tool.path_mode(), PathMode::Allowed);

        // Verify context returns correct path mode
        match tool.context() {
            ToolPrompt::Read { path_mode, .. } => {
                assert_eq!(path_mode, PathMode::Allowed);
            }
            _ => panic!("Expected ToolPrompt::Read"),
        }
    }

    #[test]
    fn uses_correct_metadata_for_absolute_resolver() {
        let tool = ReadTool::new(AbsolutePathResolver);
        let def: ToolDefinition = Tool::<()>::definition(&tool);

        // Verify the description is the absolute variant
        assert!(def.description.contains("Read a file"));
        assert!(!def.description.contains("allowed"));

        // Verify the schema uses absolute parameter
        let schema = &def.parameters_json_schema;
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let file_path = props.get("file_path").unwrap();
        let description = file_path.get("description").unwrap().as_str().unwrap();
        assert_eq!(description, "Absolute file path.");
    }

    #[test]
    fn uses_correct_metadata_for_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = ReadTool::new(resolver);
        let def: ToolDefinition = Tool::<()>::definition(&tool);

        // Verify the description is the allowed variant
        assert!(def.description.contains("allowed"));

        // Verify the schema uses allowed parameter
        let schema = &def.parameters_json_schema;
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let file_path = props.get("file_path").unwrap();
        let description = file_path.get("description").unwrap().as_str().unwrap();
        assert!(description.contains("allowed directory"));
    }
}
