//! Shared-context SerdesAI runtime builder.
//!
//! # Public API
//! - [`AgentBuildContext`] - Reusable shared inputs for building runnable agents.
//! - [`HookedAgent`] - Built agent wrapper that dispatches through run hooks.

#[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
use super::build::Profile;
use super::build::{AgentBuildError, attach_standard_tools, prepare_build};
use crate::task::TaskHandle;
use futures::Stream;
use reloaded_code_agents::AgentRuntime;
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use reloaded_code_bubblewrap::{CreateSandboxError, Preset, Profile, TempSandboxDirs};
use reloaded_code_core::hooks::{
    EndReason, HookRunContext, HookSet, PreambleRole, RunConfig, RunExecutor, RunHookFuture,
    RunOutput, RunUsage,
};
use reloaded_code_core::{CredentialLookup, CredentialResolver, models::ModelCatalog};
use serdes_ai::{Agent, AgentBuilder};
#[cfg(any(test, feature = "mock"))]
use serdes_ai_models::BoxedModel;
use std::path::Path;
use std::pin::Pin;
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

/// Lightweight newtype around a built SerdesAI `Agent` that dispatches
/// `run()` and `run_stream()` through the core `HookSet::dispatch_run` hook
/// chain when run hooks are registered. Passes through directly when no hooks
/// are present for zero overhead.
pub struct HookedAgent {
    inner: Agent<(), String>,
    hooks: HookSet,
    agent_name: String,
    model_name: String,
}

/// Result type returned by `HookedAgent::run`. Provides `.output()` and
/// `.into_output()` so existing call sites compile without changes.
pub struct HookedAgentRunResult {
    content: String,
}

/// RunExecutor that calls the inner SerdesAI agent synchronously (non-stream).
///
/// Applies `RunConfig::preamble_messages` and `system_prompt` to the prompt
/// text before calling the agent, because the built agent does not support
/// runtime mutation of those fields.
struct SerdesRunExecutor<'a> {
    agent: &'a Agent<(), String>,
    prompt: String,
    deps: (),
}

/// Shared owned state for builds that may happen later during Task delegation.
#[derive(Clone)]
pub(crate) struct TaskBuildContext<C: CredentialLookup + Send + Sync + ?Sized = CredentialResolver>
{
    runtime: Arc<AgentRuntime>,
    model_catalog: Arc<ModelCatalog>,
    credentials: Arc<C>,
    workspace_root: Arc<Path>,
    #[cfg(any(test, feature = "mock"))]
    model_override: Option<BoxedModel>,
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    bash_sandbox: Option<Arc<Profile>>,
    #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
    _sandbox_tmpdir: Option<Arc<TempSandboxDirs>>,
}

impl<C> AgentBuildContext<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    /// Creates a shared build context without a sandbox.
    ///
    /// [`BashTool`] will run commands directly on the host.
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
    ///
    /// [`BashTool`]: crate::BashTool
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
                #[cfg(any(test, feature = "mock"))]
                model_override: None,
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
    /// - `profile`: Pre-built sandbox profile for [`BashTool`].
    /// - `sandbox_tmpdir`: Optional owning temp directories that keep the
    ///   profile's backing storage alive for the context's lifetime.
    ///
    /// # Platform
    ///
    /// Only available on Linux with the `linux-bubblewrap` feature enabled.
    ///
    /// [`BashTool`]: crate::BashTool
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
    /// - `Ok(`[`HookedAgent`]`)`: A fully constructed agent ready to run.
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
    /// - Returns [`AgentBuildError::CustomToolNameMismatch`] when a custom
    ///   tool's name does not match its catalog entry name.
    /// - Returns [`AgentBuildError::CustomToolCreateFailed`] when a
    ///   custom-tool factory cannot create its portable tool object.
    #[inline]
    pub fn build(&self, name: &str) -> Result<HookedAgent, AgentBuildError> {
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

    /// Sets a mock model that overrides the resolved catalog model.
    ///
    /// # Arguments
    /// - `model`: Any [`serdes_ai_models::Model`] implementation to use instead
    ///   of the catalog-resolved model.
    ///
    /// # Returns
    /// `Self` for chaining.
    ///
    /// # Panics
    /// Panics if the [`AgentBuildContext`] has already been cloned (i.e., the
    /// inner `Arc` is not unique). This must be called before sharing the context.
    #[cfg(any(test, feature = "mock"))]
    pub fn with_model_override(mut self, model: impl serdes_ai_models::Model + 'static) -> Self {
        Arc::get_mut(&mut self.context)
            .expect("with_model_override must be called before sharing the context")
            .model_override = Some(Arc::new(model));
        self
    }
}

impl HookedAgent {
    /// Creates the wrapper from a built agent and its runtime metadata.
    pub(crate) fn new(
        inner: Agent<(), String>,
        hooks: HookSet,
        agent_name: String,
        model_name: String,
    ) -> Self {
        Self {
            inner,
            hooks,
            agent_name,
            model_name,
        }
    }

    /// Returns attached tool definitions (delegates to inner agent).
    pub fn tools(&self) -> Vec<&serdes_ai::ToolDefinition> {
        self.inner.tools()
    }

    /// Runs the agent with the given prompt, dispatching through run hooks.
    ///
    /// When no run hooks are registered this delegates directly to the inner
    /// agent for zero overhead. Otherwise it builds a `RunConfig`, runs the
    /// hook chain, applies any `preamble_messages` or `system_prompt`
    /// mutations to the prompt text, and returns the result.
    ///
    /// # Errors
    ///
    /// - Returns [`serdes_ai::agent::AgentRunError`] when the inner agent fails to complete a run.
    /// - Returns [`serdes_ai::agent::AgentRunError::Other`] when a registered run hook returns
    ///   an error during dispatch.
    pub async fn run(
        &self,
        prompt: impl Into<String>,
        deps: (),
    ) -> Result<HookedAgentRunResult, serdes_ai::agent::AgentRunError> {
        let prompt = prompt.into();
        if self.hooks.run_hooks_is_empty() {
            let response = self.inner.run(prompt, deps).await?;
            return Ok(HookedAgentRunResult::from_response(response));
        }

        let ctx = HookRunContext {
            agent_name: &self.agent_name,
            run_id: "",
            model_name: &self.model_name,
        };
        let config = RunConfig::default();

        let executor = SerdesRunExecutor {
            agent: &self.inner,
            prompt,
            deps,
        };

        let output = self
            .hooks
            .dispatch_run(&ctx, config, &executor)
            .await
            .map_err(|e| {
                serdes_ai::agent::AgentRunError::Other(anyhow::anyhow!("run hook error: {e}"))
            })?;

        Ok(HookedAgentRunResult::from_run_output(output))
    }

    /// Runs the agent in streaming mode.
    ///
    /// When no run hooks are registered this delegates directly to the inner
    /// agent's `run_stream`. When hooks are present it reuses [`Self::run`]
    /// (which already dispatches through the hook chain) and emits a synthetic
    /// stream containing the final text output.
    ///
    /// # Errors
    ///
    /// - Returns [`serdes_ai::agent::AgentRunError`] when the inner agent stream fails.
    /// - Returns [`serdes_ai::agent::AgentRunError::Other`] when a registered run hook returns
    ///   an error during dispatch.
    pub async fn run_stream(
        &self,
        prompt: impl Into<serdes_ai::core::UserContent>,
        deps: (),
    ) -> Result<
        Pin<
            Box<
                dyn Stream<
                        Item = Result<serdes_ai::AgentStreamEvent, serdes_ai::agent::AgentRunError>,
                    > + Send,
            >,
        >,
        serdes_ai::agent::AgentRunError,
    > {
        let prompt = prompt.into();
        if self.hooks.run_hooks_is_empty() {
            let stream = self.inner.run_stream(prompt, deps).await?;
            return Ok(Box::pin(stream));
        }
        let result = self
            .run(prompt.as_text().unwrap_or("").to_string(), deps)
            .await?;
        let text = result.output().to_string();
        let events = vec![
            Ok(serdes_ai::AgentStreamEvent::TextDelta { text: text.clone() }),
            Ok(serdes_ai::AgentStreamEvent::OutputReady),
            Ok(serdes_ai::AgentStreamEvent::RunComplete {
                run_id: String::new(),
                messages: Vec::new(),
            }),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

impl HookedAgentRunResult {
    /// Returns the text output.
    pub fn output(&self) -> &str {
        &self.content
    }
    /// Consumes self and returns the owned text output.
    pub fn into_output(self) -> String {
        self.content
    }

    fn from_response(response: serdes_ai::agent::AgentRunResult<String>) -> Self {
        Self {
            content: response.output().to_string(),
        }
    }

    fn from_run_output(output: RunOutput) -> Self {
        Self {
            content: output.content,
        }
    }
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
    /// - `bash_sandbox`: Pre-built sandbox profile for [`BashTool`].
    /// - `_sandbox_tmpdir`: Optional owning temp directories that keep the
    ///   profile's backing storage alive.
    ///
    /// [`BashTool`]: crate::BashTool
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
            #[cfg(any(test, feature = "mock"))]
            model_override: None,
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
            #[cfg(any(test, feature = "mock"))]
            model_override: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            bash_sandbox: None,
            #[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
            _sandbox_tmpdir: None,
        }
    }
}

impl<'a> RunExecutor for SerdesRunExecutor<'a> {
    fn execute<'b>(&'b self, _ctx: &'b HookRunContext<'b>, config: RunConfig) -> RunHookFuture<'b> {
        let agent = self.agent;
        let mut prompt = self.prompt.clone();

        // Apply RunConfig modifications that can be expressed by prepending
        // to the prompt text.
        if let Some(sys) = &config.system_prompt {
            prompt = format!("{sys}\n\n{prompt}");
        }
        for msg in &config.preamble_messages {
            match msg.role {
                PreambleRole::System => prompt = format!("[System] {}\n\n{}", msg.content, prompt),
                PreambleRole::User => prompt = format!("[User] {}\n\n{}", msg.content, prompt),
            }
        }

        #[allow(clippy::let_unit_value)]
        let deps = self.deps;
        #[allow(clippy::unit_arg)]
        Box::pin(async move {
            let response = agent
                .run(prompt, deps)
                .await
                .map_err(|e| reloaded_code_core::ToolError::Execution(e.to_string()))?;
            Ok(RunOutput {
                content: response.output().to_string(),
                reason: EndReason::Completed,
                usage: RunUsage::default(),
            })
        })
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
/// - `Ok(`[`HookedAgent`]`)`: A fully constructed agent ready to run.
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
/// - Returns [`AgentBuildError::CustomToolNameMismatch`] when a custom
///   tool's name does not match its catalog entry name.
/// - Returns [`AgentBuildError::CustomToolCreateFailed`] when a custom-tool
///   factory cannot create its portable tool object.
pub(crate) fn build_agent<C>(
    context: Arc<TaskBuildContext<C>>,
    name: &str,
    current_depth: u8,
) -> Result<HookedAgent, AgentBuildError>
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
    // Create an AgentBuilder with the model (override wins over catalog-resolved).
    #[cfg(any(test, feature = "mock"))]
    let model = context
        .model_override
        .clone()
        .unwrap_or_else(|| prepared.model().clone());
    #[cfg(not(any(test, feature = "mock")))]
    let model = prepared.model().clone();
    let builder = AgentBuilder::<(), String>::from_arc(model);
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
        context.runtime().hooks(),
    )?;
    let agent = builder.system_prompt(prompt_builder.build()).build();
    let hooks = context.runtime().hooks().clone();
    let model_name = prepared.model().name().to_string();
    Ok(HookedAgent::new(agent, hooks, name.to_string(), model_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::test_stubs::{
        agent, allow_tools, catalog, credentials, pattern_task, workspace_root,
    };
    use crate::mock::two_tools_then_text;
    use reloaded_code_agents::{AgentCatalog, AgentDefaults, AgentMode, AgentRuntimeBuilder};
    use reloaded_code_core::ToolOutput;
    use reloaded_code_core::hooks::{
        ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest,
    };
    use reloaded_code_core::permissions::{ExpandError, PermissionAction};
    use reloaded_code_core::tool_metadata::{
        read as read_meta, task as task_meta, write as write_meta,
    };
    use serde_json::json;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::TempDir;

    type TestResult = Result<(), ExpandError>;

    #[test]
    fn build_agent_skips_task_tool_when_no_targets_are_callable() -> TestResult {
        let credentials = Arc::new(credentials());
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

        let context = Arc::new(TaskBuildContext::new_for_test(
            Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root(),
        ));

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        Ok(())
    }

    #[test]
    fn build_agent_attaches_task_when_callable_targets_exist() -> TestResult {
        let credentials = Arc::new(credentials());
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

        let context = Arc::new(TaskBuildContext::new_for_test(
            Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root(),
        ));

        let agent = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
        Ok(())
    }

    #[test]
    fn build_agent_attaches_task_according_to_task_permission() -> TestResult {
        // Task permission absent: delegation defaults to every non-Primary
        // target, so the Task tool attaches alongside the allowed `read`.
        let credentials = Arc::new(credentials());
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

        let context = Arc::new(TaskBuildContext::new_for_test(
            Arc::new(runtime),
            model_catalog.clone(),
            credentials.clone(),
            workspace_root(),
        ));

        let built = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = built.tools().iter().map(|t| t.name()).collect();
        assert!(names.contains(&read_meta::NAME));
        assert!(names.contains(&task_meta::NAME));

        // Pattern-scoped Task permission: only the `reader` target is
        // callable and no other tool is allowed, so Task attaches alone.
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

        let context = Arc::new(TaskBuildContext::new_for_test(
            Arc::new(runtime),
            model_catalog.clone(),
            credentials.clone(),
            workspace_root(),
        ));

        let built = build_agent(context, "caller", 0).expect("build should succeed");
        let names: Vec<_> = built.tools().iter().map(|t| t.name()).collect();
        assert_eq!(names, vec![task_meta::NAME]);
        Ok(())
    }

    #[test]
    fn build_agent_omits_task_tool_at_max_depth() -> TestResult {
        let credentials = Arc::new(credentials());
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

        let context = Arc::new(TaskBuildContext::new_for_test(
            Arc::new(runtime),
            model_catalog,
            credentials,
            workspace_root(),
        ));

        let agent = build_agent(context, "caller", 1).expect("build should succeed");
        let names: Vec<_> = agent.tools().iter().map(|t| t.name()).collect();
        assert!(!names.contains(&task_meta::NAME));
        assert!(names.contains(&read_meta::NAME));
        Ok(())
    }

    /// Denies `write` calls to files the run has not `read`.
    ///
    /// The shared hook instance fires for every tool call of the run, so the
    /// set of read files lives behind interior mutability. Reads record their
    /// target and continue to the real tool; writes to never-read targets get
    /// an explanatory result without calling `original`, which is what
    /// short-circuits the real tool.
    struct ReadBeforeWriteHook {
        read_files: Mutex<HashSet<PathBuf>>,
    }

    impl ToolHook for ReadBeforeWriteHook {
        fn hook<'a>(
            &'a self,
            ctx: &'a ToolCallContext<'a>,
            req: ToolRequest,
            original: ToolOriginal<'a>,
        ) -> ToolHookFuture<'a> {
            Box::pin(async move {
                let target = req
                    .args
                    .get("file_path")
                    .and_then(|value| value.as_str())
                    .map(PathBuf::from);

                match (ctx.tool_name, target) {
                    (read_meta::NAME, Some(path)) => {
                        // Record the read before continuing so the lock is
                        // never held across the await.
                        self.read_files
                            .lock()
                            .expect("read_files should not be poisoned")
                            .insert(path);
                        original.call(ctx, req).await
                    }
                    (write_meta::NAME, Some(path)) => {
                        let was_read = self
                            .read_files
                            .lock()
                            .expect("read_files should not be poisoned")
                            .contains(&path);
                        if was_read {
                            return original.call(ctx, req).await;
                        }
                        // Skipping `original` blocks the call: the real
                        // `write` sits behind it and is never reached.
                        Ok(ToolOutput::new(format!(
                            "[blocked by hook] write to {} denied: no read of that file \
                             happened this run",
                            path.display()
                        )))
                    }
                    _ => original.call(ctx, req).await,
                }
            })
        }
    }

    #[tokio::test]
    async fn tool_hook_denies_write_to_never_read_file_during_agent_run() {
        // Workspace fixture: the model reads `service.env`, then tries to
        // write `draft.md`, a file the run never reads.
        let workspace = TempDir::new().expect("create temp workspace");
        let read_file = workspace.path().join("service.env");
        std::fs::write(&read_file, "LOG_LEVEL=debug\n").expect("write read fixture");
        let unread_target = workspace.path().join("draft.md");

        let hooks = HookSet::builder()
            .tool_hook(ReadBeforeWriteHook {
                read_files: Mutex::new(HashSet::new()),
            })
            .build();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[read_meta::NAME, write_meta::NAME]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        // Script the model: first turn reads the fixture, second turn writes
        // the never-read target, final turn echoes the collected tool returns.
        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            Arc::from(workspace.path()),
        )
        .with_model_override(two_tools_then_text(
            (read_meta::NAME, json!({"file_path": "service.env"})),
            (
                write_meta::NAME,
                json!({"file_path": "draft.md", "content": "Draft notes."}),
            ),
            "Run finished.",
        ));
        let hooked = context.build("caller").expect("build should succeed");

        let result = hooked
            .run("Read the config, then write the draft.", ())
            .await
            .expect("run should complete");

        // The real `read` executed: its result is the only source of the
        // fixture content in the final answer.
        assert!(
            result.output().contains("LOG_LEVEL=debug"),
            "real read result should reach the model: {}",
            result.output()
        );

        // The denied `write` never executed; the hook's response reached the
        // model in its place and the target file was never created.
        assert!(
            result.output().contains("[blocked by hook]"),
            "hook denial should reach the model: {}",
            result.output()
        );
        assert!(
            !unread_target.exists(),
            "the denied write must not create {}",
            unread_target.display()
        );
    }
}
