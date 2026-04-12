//! Shared SerdesAI agent build helpers.
//!
//! [`AgentBuildContext`](crate::agent_runtime::AgentBuildContext) and Task delegation
//! internals reuse these helpers to resolve models, permissions, and tool
//! attachments.

use super::model::resolve_model;
use super::provider_bridge::build_serdes_model;
use crate::agent_ext::{AgentBuilderExt, ToolResultExt};
use crate::task::{TaskHandle, TaskTool};
use crate::{
    BashTool, EditTool, GlobTool, GrepTool, ReadTool, SystemPromptBuilder, WebFetchTool, WriteTool,
    create_todo_tools,
};
use indexmap::IndexMap;
use llm_coding_tools_agents::{
    AgentRuntime, AgentToolSettings, ModelResolutionError, PermissionRule, TaskTargetSummary,
    ToolCatalogEntry, ToolCatalogKind, build_resolver_for_tool,
};
use llm_coding_tools_core::permissions::Ruleset;
use llm_coding_tools_core::tool_metadata::{
    edit as edit_meta, glob as glob_meta, grep as grep_meta, read as read_meta,
    webfetch as webfetch_meta, write as write_meta,
};
use llm_coding_tools_core::tools::{
    GlobSettings, GrepFormattingSettings, GrepSettings, ReadSettings, WebFetchSettings,
};
use llm_coding_tools_core::{CredentialLookup, models::ModelCatalog};
use serdes_ai::AgentBuilder;
use serdes_ai_models::BoxedModel;
use std::path::Path;
use std::sync::Arc;

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use llm_coding_tools_bubblewrap::Profile;

#[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
/// Placeholder type so [`attach_standard_tools`] compiles without the feature.
pub(super) struct Profile;

/// Error returned when a build cannot produce a SerdesAI agent.
#[derive(Debug, thiserror::Error)]
pub enum AgentBuildError {
    /// The requested agent name was not found in the runtime catalog.
    #[error("unknown agent `{name}`")]
    UnknownAgent {
        /// The missing agent name.
        name: Box<str>,
    },
    /// The runtime contains a tool kind this adapter cannot materialise.
    #[error("tool `{name}` is not supported")]
    UnsupportedToolKind {
        /// The unsupported tool name.
        name: Box<str>,
    },
    /// Resolving or validating the model configuration failed.
    #[error(transparent)]
    ModelResolution(#[from] ModelResolutionError),
    /// Initializing the SerdesAI model failed.
    #[error("failed to initialise model: {0}")]
    ModelInit(#[from] serdes_ai_models::ModelError),
    /// Tool settings validation failed during agent build.
    #[error("invalid settings for tool `{tool}`: {source}")]
    ToolSettingsValidation {
        /// The tool name that had invalid settings.
        tool: &'static str,
        /// The underlying Core tool error.
        #[source]
        source: llm_coding_tools_core::ToolError,
    },
}

/// Resolved build parameters ready for agent construction.
pub(super) struct PreparedBuild<'a> {
    /// Agent name for [`AgentBuilder::name`].
    agent_name: Box<str>,
    /// Concrete SerdesAI model.
    model: BoxedModel,
    /// Normalised SerdesAI `provider:model` specification for diagnostics.
    #[cfg_attr(not(test), allow(dead_code))]
    model_spec: Box<str>,
    /// Agent system prompt template.
    prompt: Box<str>,
    /// Sampling temperature, if specified at agent or defaults level.
    temperature: Option<f64>,
    /// Top-p sampling parameter, if specified at agent or defaults level.
    top_p: Option<f64>,
    /// Permission-filtered tool entries to materialise.
    tools: Vec<ToolCatalogEntry>,
    /// Tool settings controlling tool behaviour.
    tool_settings: AgentToolSettings,
    /// Pre-computed callable Task target summaries for the Task tool description.
    callable_target_summaries: Vec<TaskTargetSummary>,
    /// Pre-built permission ruleset for tool access control.
    /// None if agent has no permissions (backward compatibility).
    permission: Option<Arc<Ruleset>>,
    /// Raw permission config for file-tool resolver selection.
    permission_config: &'a IndexMap<String, PermissionRule>,
}

impl PreparedBuild<'_> {
    /// Returns the resolved SerdesAI model for builder construction.
    #[inline]
    pub(super) fn model(&self) -> &BoxedModel {
        &self.model
    }
}

/// Resolves model configuration and collects build parameters for an agent.
pub(super) fn prepare_build<'a, C>(
    runtime: &'a AgentRuntime,
    name: &str,
    model_catalog: &ModelCatalog,
    credentials: &C,
    with_summaries: bool,
) -> Result<PreparedBuild<'a>, AgentBuildError>
where
    C: CredentialLookup,
{
    let agent = runtime
        .catalog()
        .by_name(name)
        .ok_or_else(|| AgentBuildError::UnknownAgent { name: name.into() })?;
    let resolved = resolve_model(model_catalog, runtime.defaults(), agent)?;
    let serdes_model = build_serdes_model(model_catalog, &resolved, credentials)?;
    let tools = runtime.allowed_tools(name).to_vec();
    let callable_target_summaries = if with_summaries {
        runtime.summarize_callable_targets(name).to_vec()
    } else {
        Vec::new()
    };

    let permission = runtime
        .permission_ruleset(name)
        .filter(|ruleset| !ruleset.is_empty());

    Ok(PreparedBuild {
        agent_name: agent.name.clone(),
        model: serdes_model.model,
        model_spec: serdes_model.spec,
        prompt: agent.prompt.clone(),
        temperature: agent
            .temperature
            .or(runtime.defaults().temperature)
            .map(f64::from),
        top_p: agent.top_p.or(runtime.defaults().top_p).map(f64::from),
        tools,
        tool_settings: agent.tool_settings.clone(),
        callable_target_summaries,
        permission,
        permission_config: &agent.permission,
    })
}

/// Attaches the standard runtime tools and prompt contexts without finalizing the builder.
pub(super) fn attach_standard_tools<'a, C>(
    mut builder: AgentBuilder<(), String>,
    prepared: &PreparedBuild<'a>,
    task_handle: Option<&TaskHandle<C>>,
    workspace_root: &Path,
    bash_sandbox: Option<&Arc<Profile>>,
) -> Result<(AgentBuilder<(), String>, SystemPromptBuilder), AgentBuildError>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    // Suppress unused-variable warning for bash_sandbox in non-feature builds.
    #[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
    let _ = bash_sandbox;
    let mut prompt_builder = SystemPromptBuilder::new().system_prompt(prepared.prompt.as_ref());
    let (todo_read, todo_write, _todo_state) = create_todo_tools();

    builder = builder.name(prepared.agent_name.as_ref());
    if let Some(temperature) = prepared.temperature {
        builder = builder.temperature(temperature);
    }
    if let Some(top_p) = prepared.top_p {
        builder = builder.top_p(top_p);
    }

    // Use pre-built permission ruleset from PreparedBuild for non-file tools.
    let permission = prepared.permission.clone();
    let permission_config = &prepared.permission_config;

    for entry in prepared.tools.iter() {
        match entry.kind {
            ToolCatalogKind::Read => {
                let resolver =
                    build_resolver_for_tool(permission_config, read_meta::NAME, workspace_root)
                        .with_tool(read_meta::NAME)?;
                let settings = build_read_settings(&prepared.tool_settings.read)?;
                builder =
                    builder.tool(prompt_builder.track(ReadTool::with_settings(resolver, settings)));
            }
            ToolCatalogKind::Write => {
                let resolver =
                    build_resolver_for_tool(permission_config, write_meta::NAME, workspace_root)
                        .with_tool(write_meta::NAME)?;
                builder = builder.tool(prompt_builder.track(WriteTool::new(resolver)));
            }
            ToolCatalogKind::Edit => {
                let resolver =
                    build_resolver_for_tool(permission_config, edit_meta::NAME, workspace_root)
                        .with_tool(edit_meta::NAME)?;
                builder = builder.tool(prompt_builder.track(EditTool::new(resolver)));
            }
            ToolCatalogKind::Glob => {
                let resolver =
                    build_resolver_for_tool(permission_config, glob_meta::NAME, workspace_root)
                        .with_tool(glob_meta::NAME)?;
                let settings = build_glob_settings(&prepared.tool_settings.glob)?;
                builder =
                    builder.tool(prompt_builder.track(GlobTool::with_settings(resolver, settings)));
            }
            ToolCatalogKind::Grep => {
                let resolver =
                    build_resolver_for_tool(permission_config, grep_meta::NAME, workspace_root)
                        .with_tool(grep_meta::NAME)?;
                let (search_settings, formatting_settings) =
                    build_grep_settings(&prepared.tool_settings.grep)?;
                builder = builder.tool(prompt_builder.track(GrepTool::with_settings(
                    resolver,
                    search_settings,
                    formatting_settings,
                )));
            }
            ToolCatalogKind::Bash => {
                let settings = &prepared.tool_settings.bash;
                #[allow(unused_mut)]
                let mut tool = BashTool::new()
                    .with_timeouts(Some(settings.timeout_ms), Some(settings.max_timeout_ms))
                    .with_permission(permission.clone());
                #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
                if let Some(profile) = bash_sandbox {
                    tool = tool.with_linux_bwrap(profile.clone());
                }
                builder = builder.tool(prompt_builder.track(tool));
            }
            ToolCatalogKind::WebFetch => {
                let settings = build_webfetch_settings(&prepared.tool_settings.webfetch)?;
                builder = builder.tool(prompt_builder.track(WebFetchTool::with_settings(settings)));
            }
            ToolCatalogKind::TodoRead => {
                builder = builder.tool(prompt_builder.track(todo_read.clone()))
            }
            ToolCatalogKind::TodoWrite => {
                builder = builder.tool(prompt_builder.track(todo_write.clone()))
            }
            ToolCatalogKind::Task => {
                if let Some(task_handle) = task_handle
                    && !prepared.callable_target_summaries.is_empty()
                {
                    builder = builder.tool(prompt_builder.track(TaskTool::new(
                        prepared.agent_name.as_ref(),
                        prepared.callable_target_summaries.clone(),
                        (*task_handle).clone(),
                    )));
                }
            }
            _ => {
                return Err(AgentBuildError::UnsupportedToolKind {
                    name: entry.name.into(),
                });
            }
        }
    }

    Ok((builder, prompt_builder))
}

fn build_read_settings(
    settings: &llm_coding_tools_agents::ReadToolSettings,
) -> Result<ReadSettings, AgentBuildError> {
    ReadSettings::new()
        .with_limits(settings.limit, settings.limit)
        .and_then(|value| value.with_max_line_length(settings.max_line_length))
        .map(|value| value.with_line_numbers(settings.line_numbers))
        .with_tool(read_meta::NAME)
}

fn build_grep_settings(
    settings: &llm_coding_tools_agents::GrepToolSettings,
) -> Result<(GrepSettings, GrepFormattingSettings), AgentBuildError> {
    let search_settings = GrepSettings::new()
        .with_max_limit(settings.limit)
        .with_tool(grep_meta::NAME)?;

    let formatting_settings = GrepFormattingSettings::new()
        .with_max_line_length(settings.max_line_length)
        .map(|value| value.with_line_numbers(settings.line_numbers))
        .with_tool(grep_meta::NAME)?;

    Ok((search_settings, formatting_settings))
}

fn build_glob_settings(
    settings: &llm_coding_tools_agents::GlobToolSettings,
) -> Result<GlobSettings, AgentBuildError> {
    GlobSettings::new()
        .with_limit(settings.limit)
        .with_tool(glob_meta::NAME)
}

fn build_webfetch_settings(
    settings: &llm_coding_tools_agents::WebFetchToolSettings,
) -> Result<WebFetchSettings, AgentBuildError> {
    WebFetchSettings::new()
        .with_timeouts(settings.timeout_ms, settings.max_timeout_ms)
        .and_then(|value| value.with_max_response_size(settings.max_response_size))
        .with_tool(webfetch_meta::NAME)
}

#[cfg(test)]
mod tests {
    use super::{AgentBuildError, attach_standard_tools, prepare_build};
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use llm_coding_tools_agents::{
        AgentCatalog, AgentConfig, AgentDefaults, AgentMode, AgentRuntimeBuilder,
        AgentToolSettings, PermissionRule,
    };
    use llm_coding_tools_core::CredentialResolver;
    use llm_coding_tools_core::models::{
        Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
        ProviderSource, ProviderType,
    };
    use llm_coding_tools_core::permissions::{ExpandError, PermissionAction};
    use llm_coding_tools_core::tool_metadata::{
        bash as bash_meta, glob as glob_meta, grep as grep_meta, read as read_meta,
    };
    use serdes_ai::AgentBuilder;
    use serdes_ai_models::MockModel;
    use std::collections::HashSet;

    type TestResult = Result<(), ExpandError>;

    /// Builds an agent using a mock model instead of a real one.
    fn build_with_mock(
        prepared: &super::PreparedBuild<'_>,
        name: &str,
    ) -> serdes_ai::Agent<(), String> {
        let workspace_root =
            llm_coding_tools_core::resolve_workspace_root().expect("workspace root");
        let (builder, prompt_builder) = attach_standard_tools::<CredentialResolver>(
            AgentBuilder::<(), String>::new(MockModel::new(name)),
            prepared,
            None,
            &workspace_root,
            None,
        )
        .expect("build should succeed");
        builder.system_prompt(prompt_builder.build()).build()
    }

    /// Creates a minimal agent config with no model or sampling overrides.
    fn agent(
        name: &str,
        permission: IndexMap<String, PermissionRule>,
        prompt: &str,
    ) -> AgentConfig {
        AgentConfig {
            name: name.into(),
            mode: AgentMode::Primary,
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

    /// Creates an agent config with explicit model and sampling settings.
    fn agent_with_sampling(
        name: &str,
        model: &str,
        temperature: Option<f32>,
        top_p: Option<f32>,
        permission: IndexMap<String, PermissionRule>,
        prompt: &str,
    ) -> AgentConfig {
        AgentConfig {
            name: name.into(),
            mode: AgentMode::All,
            description: format!("{name} description").into(),
            model: Some(model.into()),
            hidden: false,
            temperature,
            top_p,
            permission,
            options: AHashMap::new(),
            tool_settings: AgentToolSettings::default(),
            prompt: prompt.into(),
        }
    }

    /// Creates permission rules that allow the specified tools.
    fn allow_tools(names: &[&str]) -> IndexMap<String, PermissionRule> {
        names
            .iter()
            .map(|n| ((*n).into(), PermissionRule::Action(PermissionAction::Allow)))
            .collect()
    }

    /// Creates a model catalog with two OpenRouter models for testing.
    fn catalog() -> ModelCatalog {
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

    fn credentials() -> CredentialResolver<false> {
        let mut credentials = CredentialResolver::without_env();
        credentials.set_override("OPENROUTER_API_KEY", "openrouter-key");
        credentials
    }

    #[test]
    fn build_filters_tools_by_permission() -> TestResult {
        let credentials = credentials();
        let catalog = catalog();

        // Create runtime with two agents: one with allowed tools, one with none
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "with-tools",
                    allow_tools(&[read_meta::NAME, bash_meta::NAME]),
                    "prompt",
                ),
                agent("no-tools", IndexMap::new(), "prompt"),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build()?;

        // Agent with permissions gets only the allowed tools
        let prepared = prepare_build(&runtime, "with-tools", &catalog, &credentials, true)
            .expect("should succeed");
        let agent = build_with_mock(&prepared, "with-tools");
        let names: HashSet<&str> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(read_meta::NAME));
        assert!(names.contains(bash_meta::NAME));
        assert_eq!(names.len(), 2);

        // Agent with empty permissions gets no tools
        let prepared = prepare_build(&runtime, "no-tools", &catalog, &credentials, true)
            .expect("should succeed");
        let agent = build_with_mock(&prepared, "no-tools");
        assert!(agent.tools().is_empty());
        Ok(())
    }

    #[test]
    fn build_uses_agent_model_and_sampling_over_defaults() -> TestResult {
        let credentials = credentials();
        let catalog = catalog();

        // Agent overrides model (gpt-4o) and sampling (0.4, 0.8) vs defaults (gpt-4.1-mini, 1.0, 0.95)
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent_with_sampling(
                "planner",
                "openrouter/openai/gpt-4o",
                Some(0.4),
                Some(0.8),
                allow_tools(&[read_meta::NAME]),
                "prompt",
            )]))
            .defaults(AgentDefaults {
                model: Some("openrouter/openai/gpt-4.1-mini".into()),
                temperature: Some(1.0),
                top_p: Some(0.95),
            })
            .build()?;

        // Agent-level settings win over defaults
        let prepared = prepare_build(&runtime, "planner", &catalog, &credentials, true)
            .expect("should succeed");
        assert_eq!(prepared.model_spec.as_ref(), "openrouter:openai/gpt-4o");
        assert!((prepared.temperature.unwrap() - 0.4).abs() < 1e-6);
        assert!((prepared.top_p.unwrap() - 0.8).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn build_handles_catalog_edge_cases() -> TestResult {
        let credentials = credentials();
        let catalog = catalog();

        // Unknown agent name returns clear error
        let runtime = AgentRuntimeBuilder::new()
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build()?;
        let err = prepare_build(&runtime, "missing", &catalog, &credentials, true)
            .err()
            .expect("should fail");
        assert!(matches!(err, AgentBuildError::UnknownAgent { name } if &*name == "missing"));

        // Duplicate agent names: catalog retains the last entry
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent_with_sampling(
                    "dupe",
                    "openrouter/openai/gpt-4.1-mini",
                    None,
                    None,
                    allow_tools(&[read_meta::NAME]),
                    "old",
                ),
                agent_with_sampling(
                    "dupe",
                    "openrouter/openai/gpt-4o",
                    None,
                    None,
                    allow_tools(&[glob_meta::NAME]),
                    "new",
                ),
            ]))
            .defaults(AgentDefaults::default())
            .build()?;
        let prepared =
            prepare_build(&runtime, "dupe", &catalog, &credentials, true).expect("should succeed");
        assert_eq!(prepared.model_spec.as_ref(), "openrouter:openai/gpt-4o");
        let agent = build_with_mock(&prepared, "dupe");
        assert_eq!(agent.tools().len(), 1);
        assert_eq!(agent.tools()[0].name(), glob_meta::NAME);
        Ok(())
    }

    /// Verifies that `tool_settings.line_numbers` selects the correct generic
    /// tool variant by checking tool descriptions (the only observable difference
    /// between `ReadTool::<true>` and `ReadTool::<false>`).
    #[test]
    fn build_wires_line_numbers_to_correct_tool_variant() -> TestResult {
        let credentials = credentials();
        let catalog = catalog();

        let mut without_line_numbers = AgentToolSettings::default();
        without_line_numbers.read.line_numbers = false;
        without_line_numbers.grep.line_numbers = false;

        // Agent with line_numbers=true (default)
        let runtime_true = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "numbered",
                allow_tools(&[read_meta::NAME, grep_meta::NAME]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build()?;

        let prepared = prepare_build(&runtime_true, "numbered", &catalog, &credentials, true)
            .expect("should succeed");

        let agent = build_with_mock(&prepared, "numbered");
        let tools: std::collections::HashMap<&str, &str> = agent
            .tools()
            .iter()
            .map(|t| (t.name(), t.description()))
            .collect();
        assert!(
            tools[read_meta::NAME].contains("line-numbered"),
            "read with line_numbers=true should mention line-numbered"
        );
        assert!(
            tools[grep_meta::NAME].contains("line numbers"),
            "grep with line_numbers=true should mention line numbers"
        );

        // Agent with line_numbers=false
        let runtime_false = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([AgentConfig {
                name: "raw".into(),
                mode: AgentMode::Primary,
                description: "raw agent".into(),
                model: None,
                hidden: false,
                temperature: None,
                top_p: None,
                permission: allow_tools(&[read_meta::NAME, grep_meta::NAME]),
                options: AHashMap::new(),
                tool_settings: without_line_numbers,
                prompt: "prompt".into(),
            }]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build()?;

        let prepared = prepare_build(&runtime_false, "raw", &catalog, &credentials, true)
            .expect("should succeed");
        let agent = build_with_mock(&prepared, "raw");
        let tools: std::collections::HashMap<&str, &str> = agent
            .tools()
            .iter()
            .map(|t| (t.name(), t.description()))
            .collect();

        assert!(
            !tools[read_meta::NAME].contains("line-numbered"),
            "read with line_numbers=false should not mention line-numbered"
        );
        assert!(
            !tools[grep_meta::NAME].contains("line numbers"),
            "grep with line_numbers=false should not mention line numbers"
        );
        Ok(())
    }
}
