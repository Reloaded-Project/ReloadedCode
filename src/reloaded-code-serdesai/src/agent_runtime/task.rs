//! Shared-context SerdesAI runtime builder.
//!
//! # Public API
//! - [`AgentBuildContext`] - Reusable shared inputs for building runnable agents.

use super::build::{AgentBuildError, attach_standard_tools, prepare_build};
use crate::task::TaskHandle;
use reloaded_code_agents::AgentRuntime;
use reloaded_code_core::{CredentialLookup, CredentialResolver, models::ModelCatalog};
use serdes_ai::{Agent, AgentBuilder};
use std::path::Path;
use std::sync::Arc;

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use reloaded_code_bubblewrap::{CreateSandboxError, Preset, Profile, TempSandboxDirs};

#[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
use super::build::Profile;

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
    /// Creates a shared build context without a sandbox.
    ///
    /// [`BashTool`](crate::BashTool) will run commands directly on the host.
    ///
    /// # Platform
    ///
    /// For sandboxed builds on Linux with the `linux-bubblewrap` feature, use
    /// `new_with_sandbox` or `new_with_temp_sandbox` instead.
    ///
    /// # Arguments
    /// - `runtime`: Shared agent runtime holding the catalog and defaults.
    /// - `model_catalog`: Available models for agent resolution.
    /// - `credentials`: Credential lookup used to authenticate model requests.
    /// - `workspace_root`: Project directory exposed to tools.
    pub fn new(
        runtime: Arc<AgentRuntime>,
        model_catalog: Arc<ModelCatalog>,
        credentials: Arc<C>,
        workspace_root: Arc<Path>,
    ) -> Self {
        Self {
            context: Arc::new(TaskBuildContext {
                runtime,
                model_catalog,
                credentials,
                workspace_root,
                #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
                bash_sandbox: None,
                #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
                _sandbox_tmpdir: None,
            }),
        }
    }

    /// Creates a shared build context with an explicitly-provided sandbox.
    ///
    /// Pass `sandbox_tmpdir` to tie the temp directory lifetime to this
    /// context; omit it when the backing storage is managed elsewhere.
    ///
    /// # Arguments
    /// - `runtime`: Shared agent runtime holding the catalog and defaults.
    /// - `model_catalog`: Available models for agent resolution.
    /// - `credentials`: Credential lookup used to authenticate model requests.
    /// - `workspace_root`: Project directory exposed to tools.
    /// - `profile`: Pre-built sandbox profile for [`BashTool`](crate::BashTool).
    /// - `sandbox_tmpdir`: Optional owning temp directories that keep the
    ///   profile's backing storage alive for the context's lifetime.
    ///
    /// # Platform
    ///
    /// Only available on Linux with the `linux-bubblewrap` feature enabled.
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    pub fn new_with_sandbox(
        runtime: Arc<AgentRuntime>,
        model_catalog: Arc<ModelCatalog>,
        credentials: Arc<C>,
        workspace_root: Arc<Path>,
        profile: Arc<Profile>,
        sandbox_tmpdir: Option<Arc<TempSandboxDirs>>,
    ) -> Self {
        Self {
            context: Arc::new(TaskBuildContext::new_with_sandbox(
                runtime,
                model_catalog,
                credentials,
                workspace_root,
                profile,
                sandbox_tmpdir,
            )),
        }
    }

    /// Creates a shared build context with an auto-managed temp sandbox.
    ///
    /// Convenience wrapper that creates a [`TempSandboxDirs`] and builds a
    /// sandbox profile from the given preset.
    ///
    /// # Arguments
    /// - `runtime`: Shared agent runtime holding the catalog and defaults.
    /// - `model_catalog`: Available models for agent resolution.
    /// - `credentials`: Credential lookup used to authenticate model requests.
    /// - `workspace_root`: Project directory exposed to tools.
    /// - `preset`: Sandbox preset controlling mount layout and permissions.
    ///
    /// # Returns
    /// - `Ok(`[`AgentBuildContext`]`)`: A shared context backed by the new
    ///   sandbox.
    ///
    /// # Errors
    /// - Returns [`CreateSandboxError::Dirs`] when the system temp directory or
    ///   any subdirectory cannot be created.
    /// - Returns [`CreateSandboxError::Unavailable`] when `bwrap` is not found
    ///   on `PATH` or is otherwise unusable on the host.
    /// - Returns [`CreateSandboxError::Profile`] when profile validation or
    ///   assembly fails (e.g., invalid paths, missing host shell).
    ///
    /// # Platform
    ///
    /// Only available on Linux with the `linux-bubblewrap` feature enabled.
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    pub fn new_with_temp_sandbox(
        runtime: Arc<AgentRuntime>,
        model_catalog: Arc<ModelCatalog>,
        credentials: Arc<C>,
        workspace_root: Arc<Path>,
        preset: Preset,
    ) -> Result<Self, CreateSandboxError> {
        let (profile, sandbox_tmpdir) =
            reloaded_code_bubblewrap::create_temp_sandbox(&workspace_root, preset)?;
        Ok(Self {
            context: Arc::new(TaskBuildContext::new_with_sandbox(
                runtime,
                model_catalog,
                credentials,
                workspace_root,
                profile,
                Some(sandbox_tmpdir),
            )),
        })
    }

    /// Builds a runnable SerdesAI agent for the named catalog entry.
    ///
    /// # Arguments
    /// - `name`: Catalog entry name to build.
    ///
    /// # Returns
    /// - `Ok(`[`Agent`]`)`: A fully constructed agent ready to run.
    ///
    /// # Errors
    /// - Returns [`AgentBuildError::UnknownAgent`] when `name` is not in the
    ///   runtime catalog.
    /// - Returns [`AgentBuildError::ModelResolution`] when model configuration
    ///   resolution or validation fails.
    /// - Returns [`AgentBuildError::ModelInit`] when the SerdesAI model fails to
    ///   initialise.
    /// - Returns [`AgentBuildError::ToolSettingsValidation`] when tool settings
    ///   validation fails during the build.
    /// - Returns [`AgentBuildError::UnsupportedToolKind`] when the runtime
    ///   contains a tool kind this adapter cannot materialise.
    /// - Returns [`AgentBuildError::UnknownCustomTool`] when a custom tool
    ///   entry names a tool absent from the custom-tool registry.
    /// - Returns [`AgentBuildError::CustomToolCreateFailed`] when a
    ///   custom-tool factory cannot create its portable tool object.
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
    workspace_root: Arc<Path>,
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    bash_sandbox: Option<Arc<Profile>>,
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    _sandbox_tmpdir: Option<Arc<TempSandboxDirs>>,
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

    /// Creates a task build context with an explicitly-provided sandbox.
    ///
    /// Pass `_sandbox_tmpdir` to tie the temp directory lifetime to this
    /// context; omit it when the backing storage is managed elsewhere.
    ///
    /// # Arguments
    /// - `runtime`: Shared agent runtime holding the catalog and defaults.
    /// - `model_catalog`: Available models for agent resolution.
    /// - `credentials`: Credential lookup used to authenticate model requests.
    /// - `workspace_root`: Project directory exposed to tools.
    /// - `bash_sandbox`: Pre-built sandbox profile for [`BashTool`](crate::BashTool).
    /// - `_sandbox_tmpdir`: Optional owning temp directories that keep the
    ///   profile's backing storage alive.
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    pub(crate) fn new_with_sandbox(
        runtime: Arc<AgentRuntime>,
        model_catalog: Arc<ModelCatalog>,
        credentials: Arc<C>,
        workspace_root: Arc<Path>,
        bash_sandbox: Arc<Profile>,
        _sandbox_tmpdir: Option<Arc<TempSandboxDirs>>,
    ) -> Self {
        Self {
            runtime,
            model_catalog,
            credentials,
            workspace_root,
            bash_sandbox: Some(bash_sandbox),
            _sandbox_tmpdir,
        }
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
        workspace_root: Arc<Path>,
    ) -> Self {
        Self {
            runtime,
            model_catalog,
            credentials,
            workspace_root,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        }
    }
}

/// Builds one runnable agent using the shared build context.
///
/// # Arguments
/// - `context`: Shared build context holding runtime, model catalog,
///   credentials, workspace root, and optional sandbox.
/// - `name`: Catalog entry name to build.
/// - `current_depth`: Current Task delegation depth (0 for top-level calls).
///
/// # Returns
/// - `Ok(`[`Agent`]`)`: A fully constructed agent ready to run.
///
/// # Errors
/// - Returns [`AgentBuildError::UnknownAgent`] when `name` is not in the
///   runtime catalog.
/// - Returns [`AgentBuildError::ModelResolution`] when model configuration
///   resolution or validation fails.
/// - Returns [`AgentBuildError::ModelInit`] when the SerdesAI model fails to
///   initialise.
/// - Returns [`AgentBuildError::ToolSettingsValidation`] when tool settings
///   validation fails during the build.
/// - Returns [`AgentBuildError::UnsupportedToolKind`] when the runtime
///   contains a tool kind this adapter cannot materialise.
/// - Returns [`AgentBuildError::UnknownCustomTool`] when a custom tool entry
///   names a tool absent from the custom-tool registry.
/// - Returns [`AgentBuildError::CustomToolCreateFailed`] when a custom-tool
///   factory cannot create its portable tool object.
pub(crate) fn build_agent<C>(
    context: Arc<TaskBuildContext<C>>,
    name: &str,
    current_depth: u8,
) -> Result<Agent<(), String>, AgentBuildError>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    // Check whether Task delegation summaries should be included at this depth.
    let with_summaries = context
        .runtime()
        .task_settings()
        .allows_delegation(current_depth);
    // Resolve model, tools, and prompt from the runtime catalog.
    let prepared = prepare_build(
        context.runtime.as_ref(),
        name,
        context.model_catalog.as_ref(),
        context.credentials.as_ref(),
        with_summaries,
    )?;
    // Create an AgentBuilder pre-loaded with the resolved model.
    let builder = AgentBuilder::<(), String>::from_arc(prepared.model().clone());
    // Create a TaskHandle for delegation if Task tool is attached later.
    let task_handle = TaskHandle::new(context.clone(), current_depth);
    // Select the sandbox profile (None on non-Linux or without the feature).
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    let sandbox_ref = context.bash_sandbox.as_ref();
    #[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
    let sandbox_ref: Option<&Arc<Profile>> = None;
    // Attach standard tools and build the system prompt.
    let (builder, prompt_builder) = attach_standard_tools(
        builder,
        &prepared,
        Some(&task_handle),
        &context.workspace_root,
        sandbox_ref,
        context.runtime.custom_tool_registry(),
    )?;
    Ok(builder.system_prompt(prompt_builder.build()).build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use reloaded_code_agents::{
        AgentCatalog, AgentConfig, AgentDefaults, AgentMode, AgentRuntimeBuilder,
        AgentToolSettings, PermissionRule,
    };
    use reloaded_code_core::CredentialResolver;
    use reloaded_code_core::models::{
        Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
        ProviderSource, ProviderType,
    };
    use reloaded_code_core::permissions::{ExpandError, PermissionAction};
    use reloaded_code_core::tool_metadata::{
        read as read_meta, task as task_meta, write as write_meta,
    };

    type TestResult = Result<(), ExpandError>;

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
            tool_settings: AgentToolSettings::default(),
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

    fn workspace_root() -> Arc<Path> {
        Arc::from(reloaded_code_core::resolve_workspace_root().expect("workspace root"))
    }

    #[test]
    fn build_agent_skips_task_tool_when_no_targets_are_callable() -> TestResult {
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
            .build()?;

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root: workspace_root(),
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        Ok(())
    }

    #[test]
    fn build_agent_attaches_task_when_callable_targets_exist() -> TestResult {
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
            .build()?;

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root: workspace_root(),
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
        Ok(())
    }

    #[test]
    fn build_agent_attaches_task_when_task_permission_is_target_scoped() -> TestResult {
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
            .build()?;

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root: workspace_root(),
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert_eq!(names, vec![task_meta::NAME]);
        Ok(())
    }

    #[test]
    fn build_agent_attaches_task_when_permission_task_is_absent() -> TestResult {
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
            .build()?;

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root: workspace_root(),
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        });

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(&read_meta::NAME));
        assert!(names.contains(&task_meta::NAME));
        Ok(())
    }

    #[test]
    fn agent_build_context_omits_task_tool_when_no_targets_are_callable() -> TestResult {
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
            .build()?;

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root(),
        );
        let agent = context.build("caller").expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        Ok(())
    }

    #[test]
    fn build_agent_omits_task_tool_at_max_depth() -> TestResult {
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
            .build()?;

        let context = Arc::new(TaskBuildContext {
            runtime: Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root: workspace_root(),
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        });

        let agent = build_agent(context, "caller", 1).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
        Ok(())
    }

    #[test]
    fn agent_build_context_omits_task_tool_when_max_depth_is_zero() -> TestResult {
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
            .build()?;

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root(),
        );
        let agent = context.build("caller").expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
        Ok(())
    }
}
