//! Shared-context SerdesAI runtime builder.
//!
//! # Public API
//! - [`AgentBuildContext`] - Reusable shared inputs for building runnable agents.

use super::build::{AgentBuildError, attach_standard_tools, prepare_build};
use crate::task::TaskHandle;
use llm_coding_tools_agents::AgentRuntime;
use llm_coding_tools_core::{CredentialLookup, CredentialResolver, models::ModelCatalog};
use serdes_ai::{Agent, AgentBuilder};
use std::sync::Arc;

/// Reusable shared inputs for building runnable SerdesAI agents.
///
/// Create once and call [`AgentBuildContext::build`] for each catalog agent
/// name you want to run. This build path always applies Task delegation
/// semantics; the Task tool is still attached conditionally based on
/// callable targets and `max_task_depth`.
#[derive(Clone)]
pub struct AgentBuildContext<C: CredentialLookup + Send + Sync + 'static = CredentialResolver> {
    context: Arc<TaskBuildContext<C>>,
}

impl<C> AgentBuildContext<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    /// Creates a shared build context from runtime state, model catalog, and credentials.
    #[inline]
    pub fn new(
        runtime: Arc<AgentRuntime>,
        model_catalog: Arc<ModelCatalog>,
        credentials: Arc<C>,
    ) -> Self {
        Self {
            context: Arc::new(TaskBuildContext {
                runtime,
                model_catalog,
                credentials,
            }),
        }
    }

    /// Builds a runnable SerdesAI agent for the named catalog entry.
    #[inline]
    pub fn build(&self, name: &str) -> Result<Agent<(), String>, AgentBuildError> {
        build_agent(Arc::clone(&self.context), name, 0)
    }

    /// Returns the shared runtime.
    #[inline]
    pub fn runtime(&self) -> &AgentRuntime {
        self.context.runtime()
    }

    /// Returns the shared model catalog.
    #[inline]
    pub fn model_catalog(&self) -> &ModelCatalog {
        self.context.model_catalog.as_ref()
    }

    /// Returns the shared credential lookup.
    #[inline]
    pub fn credentials(&self) -> &C {
        self.context.credentials.as_ref()
    }
}

/// Shared owned state for builds that may happen later during Task delegation.
#[derive(Clone)]
pub(crate) struct TaskBuildContext<C: CredentialLookup + Send + Sync + ?Sized = CredentialResolver>
{
    runtime: Arc<AgentRuntime>,
    model_catalog: Arc<ModelCatalog>,
    credentials: Arc<C>,
}

impl<C> TaskBuildContext<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    /// Returns a reference to the runtime.
    #[inline]
    pub(crate) fn runtime(&self) -> &AgentRuntime {
        self.runtime.as_ref()
    }
}

#[cfg(test)]
impl<C> TaskBuildContext<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    /// Creates a new task build context for testing.
    pub fn new_for_test(
        runtime: Arc<AgentRuntime>,
        model_catalog: Arc<ModelCatalog>,
        credentials: Arc<C>,
    ) -> Self {
        Self {
            runtime,
            model_catalog,
            credentials,
        }
    }
}

/// Builds one runnable agent using the shared build context.
pub(crate) fn build_agent<C>(
    context: Arc<TaskBuildContext<C>>,
    name: &str,
    current_depth: u8,
) -> Result<Agent<(), String>, AgentBuildError>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    let with_summaries = context
        .runtime()
        .task_settings()
        .allows_delegation(current_depth);
    let prepared = prepare_build(
        context.runtime.as_ref(),
        name,
        context.model_catalog.as_ref(),
        context.credentials.as_ref(),
        with_summaries,
    )?;
    let builder = AgentBuilder::<(), String>::from_arc(prepared.model().clone());
    let task_handle = TaskHandle::new(context, current_depth);
    let (builder, prompt_builder) = attach_standard_tools(builder, &prepared, Some(&task_handle))?;
    Ok(builder.system_prompt(prompt_builder.build()).build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use llm_coding_tools_agents::{
        AgentCatalog, AgentConfig, AgentDefaults, AgentMode, AgentRuntimeBuilder, PermissionRule,
    };
    use llm_coding_tools_core::CredentialResolver;
    use llm_coding_tools_core::models::{
        Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
        ProviderSource, ProviderType,
    };
    use llm_coding_tools_core::permissions::PermissionAction;
    use llm_coding_tools_core::tool_metadata::{
        read as read_meta, task as task_meta, write as write_meta,
    };

    fn agent(
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
            prompt: prompt.into(),
        }
    }

    fn allow_tools(names: &[&str]) -> IndexMap<String, PermissionRule> {
        names
            .iter()
            .map(|n| ((*n).into(), PermissionRule::Action(PermissionAction::Allow)))
            .collect()
    }

    fn pattern_task(patterns: &[(&str, PermissionAction)]) -> IndexMap<String, PermissionRule> {
        let mut map = IndexMap::new();
        for (pattern, action) in patterns {
            map.insert(pattern.to_string(), *action);
        }
        IndexMap::from([(task_meta::NAME.into(), PermissionRule::Pattern(map))])
    }

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

    fn credentials() -> Arc<CredentialResolver<false>> {
        let mut resolver = CredentialResolver::without_env();
        resolver.set_override("OPENROUTER_API_KEY", "test-key");
        Arc::new(resolver)
    }

    #[test]
    fn build_agent_skips_task_tool_when_no_targets_are_callable() {
        let credentials = credentials();
        let model_catalog = Arc::new(catalog());

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::Primary,
                    allow_tools(&[read_meta::NAME]),
                    "prompt",
                ),
                agent("other", AgentMode::Primary, allow_tools(&[]), "prompt"),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build();

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
    }

    #[test]
    fn build_agent_attaches_task_when_callable_targets_exist() {
        let credentials = credentials();
        let model_catalog = Arc::new(catalog());

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::All,
                    allow_tools(&[task_meta::NAME, read_meta::NAME]),
                    "prompt",
                ),
                agent(
                    "target",
                    AgentMode::All,
                    allow_tools(&[write_meta::NAME]),
                    "prompt",
                ),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build();

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
    }

    #[test]
    fn build_agent_attaches_task_when_task_permission_is_target_scoped() {
        let credentials = credentials();
        let model_catalog = Arc::new(catalog());

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::Primary,
                    pattern_task(&[
                        ("*", PermissionAction::Deny),
                        ("reader", PermissionAction::Allow),
                    ]),
                    "prompt",
                ),
                agent("reader", AgentMode::Subagent, allow_tools(&[]), "prompt"),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build();

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert_eq!(names, vec![task_meta::NAME]);
    }

    #[test]
    fn build_agent_attaches_task_when_permission_task_is_absent() {
        let credentials = credentials();
        let model_catalog = Arc::new(catalog());

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::Primary,
                    allow_tools(&[read_meta::NAME]),
                    "prompt",
                ),
                agent("reader", AgentMode::Subagent, allow_tools(&[]), "prompt"),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build();

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
        });

        // OpenCode-compatible default: omitting `permission.task` still exposes Task.
        // Any non-primary callable target keeps delegation available to the caller.
        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(&read_meta::NAME));
        assert!(names.contains(&task_meta::NAME));
    }

    #[test]
    fn agent_build_context_omits_task_tool_when_no_targets_are_callable() {
        let model_catalog = Arc::new(catalog());
        let credentials = credentials();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::Primary,
                    allow_tools(&[read_meta::NAME]),
                    "prompt",
                ),
                agent("other", AgentMode::Primary, allow_tools(&[]), "prompt"),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .build();

        let context = AgentBuildContext::new(Arc::new(runtime), model_catalog, credentials);
        let agent = context.build("caller").expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
    }

    #[test]
    fn build_agent_omits_task_tool_at_max_depth() {
        // Mid-chain: an already-delegated agent (depth=1) at max_task_depth=1
        // must not receive the Task tool.
        let credentials = credentials();
        let model_catalog = Arc::new(catalog());

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::All,
                    allow_tools(&[task_meta::NAME, read_meta::NAME]),
                    "prompt",
                ),
                agent(
                    "target",
                    AgentMode::All,
                    allow_tools(&[write_meta::NAME]),
                    "prompt",
                ),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .max_task_depth(1)
            .build();

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
        });

        let agent = build_agent(context, "caller", 1).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
    }

    #[test]
    fn agent_build_context_omits_task_tool_when_max_depth_is_zero() {
        // Root agent: max_task_depth=0 disables delegation entirely from the start.
        let model_catalog = Arc::new(catalog());
        let credentials = credentials();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::All,
                    allow_tools(&[task_meta::NAME, read_meta::NAME]),
                    "prompt",
                ),
                agent(
                    "target",
                    AgentMode::All,
                    allow_tools(&[write_meta::NAME]),
                    "prompt",
                ),
            ]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .max_task_depth(0)
            .build();

        let context = AgentBuildContext::new(Arc::new(runtime), model_catalog, credentials);
        let agent = context.build("caller").expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
    }
}
