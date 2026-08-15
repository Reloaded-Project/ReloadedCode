//! Shared test stubs and fixtures for agent-runtime tests.
//!
//! Fixtures here are shared by the test modules of [`crate::agent_runtime`],
//! [`crate::agent_runtime::build`], and [`crate::task`]; keep them inert (no
//! environment reads, no network) so any test module can reuse them.

use ahash::AHashMap;
use indexmap::IndexMap;
use reloaded_code_agents::{AgentConfig, AgentMode, AgentToolSettings, PermissionRule};
use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
    ProviderSource, ProviderType,
};
use reloaded_code_core::permissions::PermissionAction;
use reloaded_code_core::tool_metadata::task as task_meta;
use reloaded_code_core::{
    CredentialResolver, CustomTool, CustomToolDefinition, CustomToolFuture, ToolBuildContext,
    ToolFactory, ToolOutput, ToolResult, ToolRunContext,
};
use std::path::Path;
use std::sync::Arc;

/// A `ToolFactory` that creates a portable [`SerdesTestTool`].
///
/// `name` and `prompt` are surfaced via `ToolContext` for system-prompt
/// guidance injection. `response` is returned by the tool's `call()`.
#[derive(Debug)]
pub struct SerdesTestFactory {
    /// Tool name passed to `ToolContext::name()` and `ToolDefinition::new()`.
    pub name: &'static str,
    /// Prompt text passed to `ToolContext::context()`.
    pub prompt: &'static str,
    /// Text returned by `SerdesTestTool::call()`.
    pub response: &'static str,
}

/// A minimal portable custom tool that returns a configurable text response.
struct SerdesTestTool {
    name: &'static str,
    prompt: &'static str,
    response: &'static str,
}

impl SerdesTestFactory {
    /// Creates a new factory that produces a tool named `name`, with system-prompt
    /// guidance `prompt`, and `call()` returning `response`.
    #[inline]
    pub fn new(name: &'static str, prompt: &'static str, response: &'static str) -> Self {
        Self {
            name,
            prompt,
            response,
        }
    }
}

impl ToolContext for SerdesTestFactory {
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(self.prompt)
    }
}

impl ToolFactory for SerdesTestFactory {
    #[inline]
    fn create(&self, _ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
        Ok(Arc::new(SerdesTestTool {
            name: self.name,
            prompt: self.prompt,
            response: self.response,
        }))
    }
}

impl ToolContext for SerdesTestTool {
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(self.prompt)
    }
}

impl CustomTool for SerdesTestTool {
    #[inline]
    fn definition(&self) -> CustomToolDefinition {
        CustomToolDefinition::new(self.name, self.name)
    }

    #[inline]
    fn call<'a>(
        &'a self,
        _ctx: ToolRunContext<'a>,
        _args: serde_json::Value,
    ) -> CustomToolFuture<'a> {
        Box::pin(async move { Ok(ToolOutput::new(self.response)) })
    }
}

// ============================================================================
// Shared test fixtures
// ============================================================================

/// Builds an [`AgentConfig`] fixture with the given mode, permission rules,
/// and system prompt.
pub(crate) fn agent(
    name: &str,
    mode: AgentMode,
    permission: IndexMap<String, PermissionRule>,
    prompt: &str,
) -> AgentConfig {
    AgentConfig {
        name: name.into(),
        mode,
        description: format!("{name} description").into(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission,
        options: AHashMap::new(),
        tool_settings: AgentToolSettings::default(),
        prompt: prompt.into(),
    }
}

/// Builds permission rules that allow exactly the named tools.
pub(crate) fn allow_tools(names: &[&str]) -> IndexMap<String, PermissionRule> {
    names
        .iter()
        .map(|n| ((*n).into(), PermissionRule::Action(PermissionAction::Allow)))
        .collect()
}

/// Builds a two-model OpenRouter catalog fixture.
pub(crate) fn catalog() -> ModelCatalog {
    let providers = vec![ProviderSource::new(
        "openrouter",
        ProviderInfo {
            api_url: "https://openrouter.ai/api/v1".into(),
            env_vars: vec!["OPENROUTER_API_KEY".into()],
            api_type: ProviderType::OpenRouter,
        },
    )];
    let info = ModelInfo {
        modalities: Modality::TEXT,
        max_input: 128_000,
        max_output: 16_384,
        temperature: Some(1.0),
        top_p: Some(0.95),
    };
    let models: Vec<ProviderModelSource<'_>> =
        [("openai/gpt-4.1-mini", info), ("openai/gpt-4o", info)]
            .into_iter()
            .map(|(key, i)| ProviderModelSource::new(ProviderIdx::new(0), key, i))
            .collect();
    ModelCatalog::build(&providers, &models).expect("catalog fixture should build")
}

/// Builds a credential resolver with an inert OpenRouter override.
pub(crate) fn credentials() -> CredentialResolver<false> {
    let mut resolver = CredentialResolver::without_env();
    resolver.set_override("OPENROUTER_API_KEY", "test-key");
    resolver
}

/// Builds permission rules where the `task` tool dispatches on target-name
/// patterns; later patterns win, mirroring rule precedence.
pub(crate) fn pattern_task(
    patterns: &[(&str, PermissionAction)],
) -> IndexMap<String, PermissionRule> {
    let mut map = IndexMap::new();
    for (pattern, action) in patterns {
        map.insert(pattern.to_string(), *action);
    }
    IndexMap::from([(task_meta::NAME.into(), PermissionRule::Pattern(map))])
}

/// Resolves the repository workspace root, wrapped for context structs.
pub(crate) fn workspace_root() -> Arc<Path> {
    Arc::from(reloaded_code_core::resolve_workspace_root().expect("workspace root"))
}
