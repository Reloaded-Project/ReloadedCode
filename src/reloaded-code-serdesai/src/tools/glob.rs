//! File search by glob patterns, using any [`PathResolver`].
//!
//! Recursively walks a directory tree and returns file paths matching a
//! glob pattern. Results can be limited to cap output size.
//!
//! # Public API
//!
//! - [`GlobTool`] - adapter implementing [`Tool`] for glob search
//!
//! [`Tool`]: serdes_ai::tools::Tool

use async_trait::async_trait;
use reloaded_code_core::ToolContext;
use reloaded_code_core::context::{PathMode, ToolPrompt};
use reloaded_code_core::path::PathResolver;
use reloaded_code_core::tool_metadata::glob as glob_meta;
use reloaded_code_core::tools::{GlobOutput, GlobRequest, GlobSettings, glob_files};
use serde_json::json;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolResult, ToolReturn};

use crate::convert::core_error_to_serdes;

/// Tool for finding files matching glob patterns.
///
/// Generic over any [`PathResolver`] implementation.
#[derive(Debug, Clone)]
pub struct GlobTool<R: PathResolver + Clone> {
    definition: ToolDefinition,
    resolver: R,
    path_mode: PathMode,
    settings: GlobSettings,
}

impl<R: PathResolver + Clone> GlobTool<R> {
    /// Creates a new glob tool with the given resolver and default settings.
    ///
    /// Uses `limit` of 1000 files.
    ///
    /// # Type Parameters
    ///
    /// * `R` - A path resolver implementing [`PathResolver`].
    pub fn new(resolver: R) -> Self {
        Self::with_settings(resolver, GlobSettings::new())
    }

    /// Creates a new glob tool with custom settings.
    ///
    /// # Arguments
    ///
    /// * `resolver` - The path resolver for path validation.
    /// * `settings` - Core glob settings for result limits.
    pub fn with_settings(resolver: R, settings: GlobSettings) -> Self {
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
impl<R: PathResolver + Clone + Send + Sync, Deps: Send + Sync> Tool<Deps> for GlobTool<R> {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args =
            GlobRequest::parse(args).map_err(|e| core_error_to_serdes(glob_meta::NAME, e))?;

        let output = glob_files(&self.resolver, args, &self.settings)
            .map_err(|e| core_error_to_serdes(glob_meta::NAME, e))?;

        Ok(glob_output_to_return(output))
    }
}

const NO_FILES_FOUND: &str = "No files found matching the pattern.";

fn output_content(files: &[String]) -> String {
    if files.is_empty() {
        NO_FILES_FOUND.to_string()
    } else {
        files.join("\n")
    }
}

fn glob_output_to_return(output: GlobOutput) -> ToolReturn {
    let content = output_content(&output.files);

    if output.partial {
        return ToolReturn::json(json!({
            "content": content,
            "partial": true,
            "errors": output.errors,
            "truncated": output.truncated,
        }));
    }

    if output.truncated {
        ToolReturn::json(json!({
            "content": content,
            "truncated": true,
        }))
    } else {
        ToolReturn::text(content)
    }
}

impl<R: PathResolver + Clone> ToolContext for GlobTool<R> {
    const NAME: &'static str = glob_meta::NAME;

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Glob {
            path_mode: self.path_mode,
        }
    }
}

fn build_definition(path_mode: PathMode) -> ToolDefinition {
    let (path_param, description) = match path_mode {
        PathMode::Absolute => (
            glob_meta::param::PATH_ABSOLUTE,
            glob_meta::description::ABSOLUTE,
        ),
        PathMode::Allowed => (
            glob_meta::param::PATH_ALLOWED,
            glob_meta::description::ALLOWED,
        ),
    };

    let schema = SchemaBuilder::new()
        .string(
            glob_meta::param::PATTERN.name,
            glob_meta::param::PATTERN.description,
            glob_meta::param::PATTERN.required,
        )
        .string(path_param.name, path_param.description, path_param.required)
        .build()
        .expect("schema build should not fail");

    ToolDefinition {
        name: glob_meta::NAME.to_owned(),
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
    use reloaded_code_core::tools::GlobOutput;
    use serde_json::json;
    use serdes_ai::tools::RunContext;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn mock_ctx() -> RunContext<()> {
        RunContext::new((), "test-model")
    }

    #[tokio::test]
    async fn finds_files_with_absolute_resolver() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        File::create(dir.path().join("src/lib.rs")).unwrap();
        File::create(dir.path().join("src/main.rs")).unwrap();

        let tool = GlobTool::new(AbsolutePathResolver);
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

    #[tokio::test]
    async fn finds_matching_files_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        File::create(dir.path().join("src/lib.rs")).unwrap();

        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GlobTool::new(resolver);
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
    async fn rejects_path_traversal_with_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GlobTool::new(resolver);
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

    #[test]
    fn determines_correct_path_mode_for_absolute_resolver() {
        let tool = GlobTool::new(AbsolutePathResolver);
        assert_eq!(tool.path_mode(), PathMode::Absolute);
    }

    #[test]
    fn determines_correct_path_mode_for_allowed_resolver() {
        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new([dir.path()]).unwrap();
        let tool = GlobTool::new(resolver);
        assert_eq!(tool.path_mode(), PathMode::Allowed);
    }
}
