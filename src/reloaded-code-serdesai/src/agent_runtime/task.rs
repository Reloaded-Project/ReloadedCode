//! Shared-context SerdesAI runtime builder.
//!
//! # Public API
//! - [`AgentBuildContext`] - Reusable shared inputs for building runnable agents.
//! - [`HookedAgent`] - Built agent wrapper that dispatches `run()` through
//!   the registered run-config and run hooks, resolves run-config hooks
//!   once before `run_stream()` starts, and passes each streamed event
//!   through the registered run-event hooks.

#[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
use super::build::Profile;
use super::build::{AgentBuildError, attach_standard_tools, prepare_build};
use super::compact::{CompactModel, CompactionRecords};
use super::stream_events::RunEventStream;
use crate::task::TaskHandle;
use futures::Stream;
use reloaded_code_agents::AgentRuntime;
#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
use reloaded_code_bubblewrap::{CreateSandboxError, Preset, Profile, TempSandboxDirs};
use reloaded_code_core::hooks::{
    EndReason, HookRunContext, HookSet, ModelSettingsOverrides, PreambleRole, RunConfig, RunEvent,
    RunExecutor, RunHookFuture, RunOutput, RunUsage,
};
use reloaded_code_core::{
    CompactPolicy, CredentialLookup, CredentialResolver, models::ModelCatalog,
};
use serdes_ai::core::{UserContent, UserContentPart};
use serdes_ai::{Agent, AgentBuilder, RunOptions};
use serdes_ai_models::BoxedModel;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Prefix marking a preamble message as system-role in the prompt text.
const PREAMBLE_SYSTEM_PREFIX: &str = "[System] ";
/// Prefix marking a preamble message as user-role in the prompt text.
const PREAMBLE_USER_PREFIX: &str = "[User] ";
/// Blank line separating two prompt sections, and the section head from
/// the original prompt on both run paths.
const SECTION_SEPARATOR: &str = "\n\n";

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

/// Lightweight newtype around a built SerdesAI `Agent`.
///
/// Wraps both run paths with the runtime's registered hooks:
///
/// - [`Self::run`]: [`RunConfigHook`]s amend the run config,
///   then the run hook chain drives the run. With neither hook kind
///   registered, the inner agent runs directly.
/// - [`Self::run_stream`]: the same run-config hooks fire once before the
///   stream starts, then each mapped [`RunEvent`] passes the run-event
///   hook chain.
///
/// [`RunConfigHook`]: reloaded_code_core::hooks::RunConfigHook
pub struct HookedAgent {
    inner: Agent<(), String>,
    hooks: HookSet,
    agent_name: String,
    model_name: String,
    /// Applied compactions awaiting stream publication, present when
    /// the agent's model runs context compaction.
    compaction: Option<CompactionRecords>,
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
/// runtime mutation of those fields. Applies `model_settings_overrides` to
/// the per-run model settings via [`RunOptions`], merged over the agent's
/// configured settings.
///
/// On inner failure the hook chain sees a [`ToolError::Execution`] projection
/// while the original [`AgentRunError`] is parked in `error`; the dispatch
/// site in `run_hooked` restores the original when the failure reaches
/// the caller untouched.
///
/// [`ToolError::Execution`]: reloaded_code_core::ToolError::Execution
/// [`AgentRunError`]: serdes_ai::agent::AgentRunError
struct SerdesRunExecutor<'a> {
    agent: &'a Agent<(), String>,
    prompt: String,
    deps: (),
    /// Slot the executor fills with the inner run's original failure.
    error: Arc<Mutex<Option<serdes_ai::agent::AgentRunError>>>,
}

/// Shared owned state for builds that may happen later during Task delegation.
#[derive(Clone)]
pub(crate) struct TaskBuildContext<C: CredentialLookup + Send + Sync + ?Sized = CredentialResolver>
{
    runtime: Arc<AgentRuntime>,
    model_catalog: Arc<ModelCatalog>,
    credentials: Arc<C>,
    workspace_root: Arc<Path>,
    /// Compaction policy for builds that enable context compaction;
    /// `None` keeps every built model unwrapped.
    compaction: Option<CompactPolicy>,
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
                compaction: None,
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

    /// Enables opt-in context compaction with `policy`.
    ///
    /// Every agent built from this context wraps its model so each
    /// step request is checked against the policy's trigger
    /// threshold; over it, older history is summarized through the
    /// run's own model and a recent window stays verbatim. Start from
    /// [`CompactPolicy::default`] and override the fields that
    /// differ; the defaults trigger 32,000 tokens below the model's
    /// context limit and cap the summarize request at 32,000 output
    /// tokens.
    ///
    /// A context that skips this keeps compaction disabled: no model
    /// wrapper, no per-request estimation, no compaction events.
    ///
    /// # Arguments
    /// - `policy`: When to compact and how large the summarize request
    ///   may be.
    ///
    /// # Returns
    /// `Self` for chaining.
    ///
    /// # Panics
    /// Panics if the [`AgentBuildContext`] has already been cloned (i.e., the
    /// inner `Arc` is not unique). This must be called before sharing the context.
    pub fn with_compaction(mut self, policy: CompactPolicy) -> Self {
        Arc::get_mut(&mut self.context)
            .expect("with_compaction must be called before sharing the context")
            .compaction = Some(policy);
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
        compaction: Option<CompactionRecords>,
    ) -> Self {
        Self {
            inner,
            hooks,
            agent_name,
            model_name,
            compaction,
        }
    }

    /// Returns attached tool definitions (delegates to inner agent).
    pub fn tools(&self) -> Vec<&serdes_ai::ToolDefinition> {
        self.inner.tools()
    }

    /// Runs the agent with the given prompt, dispatching through the
    /// registered run-config and run hooks.
    ///
    /// With no run-config or run hooks registered this delegates directly
    /// to the inner agent. Otherwise [`RunConfigHook`][config-hook]s amend
    /// the run config first; the run hook chain then observes the final
    /// config.
    ///
    /// Run hooks fire only on this path. Run-config hooks fire here and
    /// on [`Self::run_stream`]. Run-event hooks
    /// ([`RunEventHook`][event-hook]) fire only on [`Self::run_stream`].
    ///
    /// The hook context carries a wrapper-generated `run_id`. The inner
    /// agent assigns its own id for tool hooks; SerdesAI `RunOptions` has
    /// no field to override it, so the two cannot be unified.
    ///
    /// # Errors
    ///
    /// - Returns the inner agent's [`serdes_ai::agent::AgentRunError`]
    ///   unchanged when the inner agent fails and the failure reaches the
    ///   caller untouched.
    /// - Returns [`serdes_ai::agent::AgentRunError::Other`] when a
    ///   run-config hook or run hook returns or substitutes its own error
    ///   during dispatch.
    /// - With no run hooks registered, a failing run-config hook is
    ///   labeled `run config hook error`.
    /// - With run hooks registered, any dispatch failure that is not the
    ///   untouched inner error is labeled `run hook error`.
    ///
    /// [event-hook]: reloaded_code_core::hooks::RunEventHook
    /// [config-hook]: reloaded_code_core::hooks::RunConfigHook
    pub async fn run(
        &self,
        prompt: impl Into<String>,
        deps: (),
    ) -> Result<HookedAgentRunResult, serdes_ai::agent::AgentRunError> {
        self.run_hooked(prompt.into(), deps).await
    }

    /// Hooked-run implementation behind `run`.
    ///
    /// # Errors
    ///
    /// - Returns the inner agent's [`serdes_ai::agent::AgentRunError`]
    ///   unchanged when the inner agent fails (direct run or hooked run)
    ///   and the failure reaches the caller untouched.
    /// - Returns [`serdes_ai::agent::AgentRunError::Other`] when a
    ///   run-config hook or run hook returns or substitutes its own error
    ///   during dispatch.
    async fn run_hooked(
        &self,
        prompt: String,
        deps: (),
    ) -> Result<HookedAgentRunResult, serdes_ai::agent::AgentRunError> {
        if self.hooks.run_hooks_is_empty() && self.hooks.run_config_hooks_is_empty() {
            let response = self.inner.run(prompt, deps).await?;
            let serdes_ai::agent::AgentRunResult { output, .. } = response;
            return Ok(HookedAgentRunResult { content: output });
        }

        // Wrapper-assigned run id for the hook context. The inner agent
        // generates its own id for the real run and tool hooks; see the
        // `run` doc comment.
        let run_id = serdes_ai::agent::generate_run_id();
        let ctx = HookRunContext {
            agent_name: &self.agent_name,
            run_id: &run_id,
            model_name: &self.model_name,
        };
        let config = RunConfig::default();

        let error_slot = Arc::new(Mutex::new(None));
        let executor = SerdesRunExecutor {
            agent: &self.inner,
            prompt,
            deps,
            error: Arc::clone(&error_slot),
        };

        // A dispatch failure carries a `ToolError`; restore the inner
        // agent's original `AgentRunError` when it propagated untouched,
        // and only label the error hook-origin otherwise.
        let output = match self.hooks.dispatch_run(&ctx, config, &executor).await {
            Ok(output) => output,
            Err(dispatched) => {
                return Err(restore_run_error(
                    dispatched,
                    &error_slot,
                    !self.hooks.run_hooks_is_empty(),
                ));
            }
        };

        Ok(HookedAgentRunResult::from_run_output(output))
    }

    /// Runs the agent in streaming mode, yielding framework-owned
    /// [`RunEvent`]s.
    ///
    /// Starts the inner agent's stream and lazily maps each vendor event
    /// as the stream is polled, so real incremental text and thinking
    /// deltas reach the caller as they arrive. The mapped
    /// [`RunEvent::RunComplete`] carries the inner run's id and a
    /// distilled transcript. The prompt accepts full [`UserContent`];
    /// image and multi-part prompts keep their parts when no sections
    /// are injected.
    ///
    /// # Hooks
    ///
    /// - **Run-event**: each mapped event passes the registered
    ///   [`RunEventHook`][event-hook] chain, in registration order,
    ///   before the caller sees it; a hook may rewrite or suppress it.
    /// - **Run-config**: [`RunConfigHook`][config-hook]s fire once
    ///   before the stream starts. The system prompt and preamble
    ///   messages become a leading text part of the prompt;
    ///   model-settings overrides merge field-wise over the
    ///   agent's configured settings.
    /// - **Run**: [`RunHook`][run-hook]s never fire here; they fire
    ///   only on [`Self::run`], because dispatching them would buffer
    ///   the whole run before the first event.
    /// - **Compaction**: when the build enables context compaction,
    ///   each applied compaction publishes one
    ///   [`RunEvent::ContextCompressed`] through the run-event chain,
    ///   after the compacted step's `ContextInfo` and before that
    ///   step's content.
    ///
    /// # Errors
    ///
    /// - Returns [`serdes_ai::agent::AgentRunError::Other`] labeled
    ///   `run config hook error: ...` when a run-config hook fails; the
    ///   failure surfaces from this start call before any event exists.
    /// - Returns the inner agent's [`serdes_ai::agent::AgentRunError`]
    ///   unchanged when starting the stream fails.
    /// - The stream yields the inner error as its final `Err` item when
    ///   the run fails mid-stream; a vendor error event maps to
    ///   [`RunEvent::Error`] before that final item.
    /// - The stream yields [`serdes_ai::agent::AgentRunError::Other`] as
    ///   its final item when a run-event hook fails; the stream ends after
    ///   that item.
    ///
    /// [event-hook]: reloaded_code_core::hooks::RunEventHook
    /// [run-hook]: reloaded_code_core::hooks::RunHook
    /// [config-hook]: reloaded_code_core::hooks::RunConfigHook
    pub async fn run_stream(
        &self,
        prompt: impl Into<serdes_ai::core::UserContent>,
        deps: (),
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<RunEvent, serdes_ai::agent::AgentRunError>> + Send>>,
        serdes_ai::agent::AgentRunError,
    > {
        // Run hooks never fire on the stream, so only the config chain
        // gates the pre-pass; an empty config chain streams the inner
        // agent directly, exactly like the unhooked path.
        if self.hooks.run_config_hooks_is_empty() {
            let inner = self.inner.run_stream(prompt, deps).await?;
            return Ok(Box::pin(RunEventStream::new(
                inner,
                &self.hooks,
                &self.agent_name,
                &self.model_name,
                self.compaction.as_ref(),
            )));
        }

        // Wrapper-assigned run id for the hook context. The inner agent
        // generates its own id for the streamed run; see the `run` doc
        // comment.
        let run_id = serdes_ai::agent::generate_run_id();
        let ctx = HookRunContext {
            agent_name: &self.agent_name,
            run_id: &run_id,
            model_name: &self.model_name,
        };

        // Config resolution completes once, before the stream starts;
        // its failure aborts the start call instead of surfacing
        // mid-stream.
        let config = self
            .hooks
            .dispatch_run_config(&ctx, RunConfig::default())
            .await
            .map_err(|dispatched| {
                serdes_ai::agent::AgentRunError::Other(anyhow::anyhow!(
                    "run config hook error: {dispatched}"
                ))
            })?;

        let prompt = prepend_section_head(prompt.into(), run_config_head(&config));
        let inner = match run_options_with_overrides(&self.inner, config.model_settings_overrides) {
            Some(options) => {
                self.inner
                    .run_stream_with_options(prompt, deps, options)
                    .await?
            }
            None => self.inner.run_stream(prompt, deps).await?,
        };
        Ok(Box::pin(RunEventStream::new(
            inner,
            &self.hooks,
            &self.agent_name,
            &self.model_name,
            self.compaction.as_ref(),
        )))
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
            compaction: None,
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
            compaction: None,
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

        // Config sections render textually before the prompt: system
        // prompt first, then preamble messages in configured order,
        // then the original prompt.
        if let Some(head) = run_config_head(&config) {
            prompt = format!("{head}{SECTION_SEPARATOR}{prompt}");
        }

        let error = Arc::clone(&self.error);
        let run_options = run_options_with_overrides(agent, config.model_settings_overrides);
        #[allow(clippy::let_unit_value)]
        let deps = self.deps;
        #[allow(clippy::unit_arg)]
        Box::pin(async move {
            let result = if let Some(options) = run_options {
                agent.run_with_options(prompt, deps, options).await
            } else {
                agent.run(prompt, deps).await
            };
            let response = match result {
                Ok(response) => response,
                Err(err) => {
                    // Hooks see the `ToolError` projection; the original is
                    // restored after dispatch when it propagates untouched.
                    let projection = run_error_projection(&err);
                    *error.lock().expect("run error slot should not be poisoned") = Some(err);
                    return Err(projection);
                }
            };
            let content = response.output().to_string();
            let usage = RunUsage {
                prompt_tokens: response.usage.request_tokens,
                completion_tokens: response.usage.response_tokens,
            };
            Ok(RunOutput {
                content,
                reason: EndReason::Completed,
                usage,
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
    // Context compaction wraps the model when the build enables it, so
    // every step request passes the policy threshold check before the
    // model serves it. A disabled build keeps its model untouched.
    let (model, compaction) = match context.compaction {
        Some(policy) => {
            let (wrapped, records) = CompactModel::new(model, policy);
            (Arc::new(wrapped) as BoxedModel, Some(records))
        }
        None => (model, None),
    };
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
    Ok(HookedAgent::new(
        agent,
        hooks,
        name.to_string(),
        model_name,
        compaction,
    ))
}

/// Prepends a config-injected head (from [`run_config_head`]) to a
/// `run_stream` prompt as its own leading text part, so the original
/// prompt content follows it unchanged.
///
/// The head goes in as a separate part because stream prompts carry
/// full [`UserContent`]: a text prompt becomes two parts, head first,
/// and a multi-part prompt keeps its parts with the head inserted at
/// index zero, so image and other parts survive. A `None` head
/// returns the prompt unchanged.
fn prepend_section_head(prompt: UserContent, head: Option<String>) -> UserContent {
    let Some(head) = head else {
        return prompt;
    };
    let head_part = UserContentPart::text(format!("{head}{SECTION_SEPARATOR}"));
    match prompt {
        UserContent::Text(text) => UserContent::Parts(vec![head_part, UserContentPart::text(text)]),
        UserContent::Parts(mut parts) => {
            parts.insert(0, head_part);
            UserContent::Parts(parts)
        }
    }
}

/// Recovers the failure from a failed run dispatch.
///
/// Restores the captured inner-agent [`AgentRunError`] when the hook chain
/// propagated its projection untouched. Any other dispatched error reached
/// the caller through a hook returning or substituting its own error, so it
/// is labeled as hook-origin. With no run hooks registered, only the
/// run-config chain can have produced the error, so it carries the
/// config-hook label; otherwise the run-hook label applies.
///
/// [`AgentRunError`]: serdes_ai::agent::AgentRunError
fn restore_run_error(
    dispatched: reloaded_code_core::ToolError,
    captured: &Mutex<Option<serdes_ai::agent::AgentRunError>>,
    run_hooks_registered: bool,
) -> serdes_ai::agent::AgentRunError {
    let captured = captured
        .lock()
        .expect("run error slot should not be poisoned")
        .take();
    match captured {
        Some(inner) if run_error_projection(&inner).to_string() == dispatched.to_string() => inner,
        _ if run_hooks_registered => {
            serdes_ai::agent::AgentRunError::Other(anyhow::anyhow!("run hook error: {dispatched}"))
        }
        _ => serdes_ai::agent::AgentRunError::Other(anyhow::anyhow!(
            "run config hook error: {dispatched}"
        )),
    }
}

/// Builds the leading text placed in front of the user's prompt when a
/// run config injects a system prompt or preamble messages.
///
/// The head starts with the system prompt, then preamble messages in
/// configured order, tagged `[System]` or `[User]` by role. A blank
/// line separates consecutive sections.
///
/// Returns `None` when the config injects nothing, leaving the prompt
/// unchanged. [`SerdesRunExecutor::execute`] and
/// [`HookedAgent::run_stream`] share this builder, so both run paths
/// prepend the same head bytes.
fn run_config_head(config: &RunConfig) -> Option<String> {
    let section_count =
        config.preamble_messages.len() + usize::from(config.system_prompt.is_some());
    if section_count == 0 {
        return None;
    }
    // Capacity: every section's content plus the longest role prefix and
    // one blank-line separator per section; a slight overestimate is
    // harmless.
    let estimated_len = (PREAMBLE_SYSTEM_PREFIX.len() + SECTION_SEPARATOR.len()) * section_count
        + config.system_prompt.as_ref().map_or(0, String::len)
        + config
            .preamble_messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>();
    let mut head = String::with_capacity(estimated_len);
    let mut first_section = true;
    if let Some(system_prompt) = &config.system_prompt {
        head.push_str(system_prompt);
        first_section = false;
    }
    for message in &config.preamble_messages {
        // One blank line between every section pair, whatever the
        // section content, so empty sections keep their separator and
        // the rendered bytes stay stable.
        if !first_section {
            head.push_str(SECTION_SEPARATOR);
        }
        first_section = false;
        let prefix = match message.role {
            PreambleRole::System => PREAMBLE_SYSTEM_PREFIX,
            PreambleRole::User => PREAMBLE_USER_PREFIX,
        };
        head.push_str(prefix);
        head.push_str(&message.content);
    }
    Some(head)
}

/// Builds per-run [`RunOptions`] that merge [`ModelSettingsOverrides`] over
/// the agent's configured settings.
///
/// Returns `None` when no override field is set, so those runs keep the plain
/// [`Agent::run`] behavior. An overridden field replaces only that field;
/// all others keep the agent's values, because a provided
/// `RunOptions::model_settings` replaces the agent's settings wholesale.
///
/// Every [`ModelSettingsOverrides`] field is bound explicitly (no rest
/// pattern), so adding a field fails compilation here; extend this function
/// to apply the new field or reject it with
/// [`ToolError::validation_for`][reloaded_code_core::ToolError::validation_for]
/// naming `model_settings_overrides`.
fn run_options_with_overrides(
    agent: &Agent<(), String>,
    overrides: Option<ModelSettingsOverrides>,
) -> Option<RunOptions> {
    let ModelSettingsOverrides { temperature, top_p } = overrides?;
    if temperature.is_none() && top_p.is_none() {
        return None;
    }
    let mut settings = agent.model_settings().clone();
    if let Some(temperature) = temperature {
        settings.temperature = Some(f64::from(temperature));
    }
    if let Some(top_p) = top_p {
        settings.top_p = Some(f64::from(top_p));
    }
    Some(RunOptions::default().model_settings(settings))
}

/// Deterministic [`ToolError`] projection of an inner-agent run failure that
/// the run hook chain carries. The dispatch site recognizes an untouched
/// projection and restores the original [`AgentRunError`] afterward.
///
/// [`ToolError`]: reloaded_code_core::ToolError
/// [`AgentRunError`]: serdes_ai::agent::AgentRunError
fn run_error_projection(err: &serdes_ai::agent::AgentRunError) -> reloaded_code_core::ToolError {
    reloaded_code_core::ToolError::Execution(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::test_stubs::{
        agent, allow_tools, catalog, credentials, pattern_task, workspace_root,
    };
    use crate::mock::{FunctionModel, two_tools_then_text};
    use reloaded_code_agents::{AgentCatalog, AgentDefaults, AgentMode, AgentRuntimeBuilder};
    use reloaded_code_core::ToolOutput;
    use reloaded_code_core::hooks::{
        ModelSettingsOverrides, PreambleMessage, PreambleRole, RunConfigHook, RunConfigHookFuture,
        RunHook, RunOriginal, ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest,
    };
    use reloaded_code_core::permissions::{ExpandError, PermissionAction};
    use reloaded_code_core::tool_metadata::{
        read as read_meta, task as task_meta, write as write_meta,
    };
    use serde_json::json;
    use serdes_ai::core::{ModelRequest, ModelResponse, ModelSettings};
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
    /// target keyed by `(run_id, path)` so authorization cannot leak across
    /// runs; writes to never-read targets get an explanatory result without
    /// calling `original`, which is what short-circuits the real tool.
    struct ReadBeforeWriteHook {
        read_files: Mutex<HashSet<(String, PathBuf)>>,
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
                            .insert((ctx.run_id.to_string(), path));
                        original.call(ctx, req).await
                    }
                    (write_meta::NAME, Some(path)) => {
                        let was_read = self
                            .read_files
                            .lock()
                            .expect("read_files should not be poisoned")
                            .contains(&(ctx.run_id.to_string(), path.clone()));
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

    /// Model settings overrides: applied per run, merged over the agent's
    /// configured settings, with prompt-prepend behavior untouched.
    ///
    /// Run-config hook that installs fixed model settings overrides.
    struct OverridingConfigHook {
        temperature: Option<f32>,
        top_p: Option<f32>,
    }

    impl RunConfigHook for OverridingConfigHook {
        fn configure<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            config: &'a mut RunConfig,
        ) -> RunConfigHookFuture<'a> {
            let temperature = self.temperature;
            let top_p = self.top_p;
            Box::pin(async move {
                config.model_settings_overrides =
                    Some(ModelSettingsOverrides { temperature, top_p });
                Ok(())
            })
        }
    }

    /// Run-config hook that injects prompt sections plus a temperature
    /// override.
    struct PromptAndSettingsOverrideConfigHook;

    impl RunConfigHook for PromptAndSettingsOverrideConfigHook {
        fn configure<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            config: &'a mut RunConfig,
        ) -> RunConfigHookFuture<'a> {
            Box::pin(async move {
                config.system_prompt = Some("agent system override".into());
                config.preamble_messages = vec![
                    PreambleMessage {
                        role: PreambleRole::System,
                        content: "sys note".into(),
                    },
                    PreambleMessage {
                        role: PreambleRole::User,
                        content: "user note".into(),
                    },
                ];
                config.model_settings_overrides = Some(ModelSettingsOverrides {
                    temperature: Some(0.9),
                    top_p: None,
                });
                Ok(())
            })
        }
    }

    /// Builds a hooked agent with agent-level settings temperature 0.3 and
    /// top_p 0.8, running a model that records the [`ModelSettings`] of every
    /// request and echoes the last user prompt.
    fn hooked_agent_with_settings_capture(
        hooks: HookSet,
    ) -> (HookedAgent, Arc<Mutex<Vec<ModelSettings>>>) {
        let captured: Arc<Mutex<Vec<ModelSettings>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&captured);
        let model = FunctionModel::new(move |messages, settings| {
            seen.lock()
                .expect("captured settings should not be poisoned")
                .push(settings.clone());
            let last_user = messages
                .iter()
                .rev()
                .flat_map(|m| m.user_prompts())
                .next()
                .and_then(|prompt| prompt.as_text())
                .unwrap_or_default()
                .to_string();
            ModelResponse::text(last_user)
        });

        let mut defaults = AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini");
        defaults.temperature = Some(0.3);
        defaults.top_p = Some(0.8);
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(defaults)
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(model);
        let hooked = context.build("caller").expect("build should succeed");
        (hooked, captured)
    }

    #[tokio::test]
    async fn model_settings_override_replaces_only_the_overridden_setting_in_request() {
        let (hooked, captured) = hooked_agent_with_settings_capture(
            HookSet::builder()
                .run_config_hook(OverridingConfigHook {
                    temperature: Some(0.9),
                    top_p: None,
                })
                .build(),
        );

        hooked.run("hello", ()).await.expect("run should complete");

        {
            let seen = captured
                .lock()
                .expect("captured settings should not be poisoned");
            assert_eq!(seen.len(), 1, "one model request should have been made");
            assert_eq!(seen[0].temperature, Some(f64::from(0.9_f32)));
            assert_eq!(
                seen[0].top_p,
                Some(f64::from(0.8_f32)),
                "agent-configured top_p should be retained"
            );
        }

        // Mirror direction: a top_p-only override replaces top_p and keeps
        // the agent-configured temperature.
        let (hooked, captured) = hooked_agent_with_settings_capture(
            HookSet::builder()
                .run_config_hook(OverridingConfigHook {
                    temperature: None,
                    top_p: Some(0.6),
                })
                .build(),
        );

        hooked.run("hello", ()).await.expect("run should complete");

        {
            let seen = captured
                .lock()
                .expect("captured settings should not be poisoned");
            assert_eq!(seen.len(), 1, "one model request should have been made");
            assert_eq!(seen[0].top_p, Some(f64::from(0.6_f32)));
            assert_eq!(
                seen[0].temperature,
                Some(f64::from(0.3_f32)),
                "agent-configured temperature should be retained"
            );
        }
    }

    #[tokio::test]
    async fn run_without_model_settings_overrides_uses_agent_configured_settings() {
        // Absent overrides: `RunConfig::default()` flows through untouched.
        let (hooked, captured) = hooked_agent_with_settings_capture(
            HookSet::builder().run_hook(PassthroughRunHook).build(),
        );
        hooked.run("hello", ()).await.expect("run should complete");

        // All-None overrides: no field is set, so agent settings apply as-is.
        let (hooked, captured_empty) = hooked_agent_with_settings_capture(
            HookSet::builder()
                .run_config_hook(OverridingConfigHook {
                    temperature: None,
                    top_p: None,
                })
                .build(),
        );
        hooked.run("hello", ()).await.expect("run should complete");

        let expected = ModelSettings {
            temperature: Some(f64::from(0.3_f32)),
            top_p: Some(f64::from(0.8_f32)),
            ..ModelSettings::default()
        };
        for run in [captured, captured_empty] {
            let seen = run
                .lock()
                .expect("captured settings should not be poisoned");
            assert_eq!(seen.len(), 1, "one model request should have been made");
            assert_eq!(
                seen[0], expected,
                "no-override runs must use the agent's configured settings"
            );
        }
    }

    #[tokio::test]
    async fn prompt_sections_are_unchanged_when_model_settings_overrides_are_present() {
        let (hooked, captured) = hooked_agent_with_settings_capture(
            HookSet::builder()
                .run_config_hook(PromptAndSettingsOverrideConfigHook)
                .build(),
        );

        let output = hooked
            .run("base prompt", ())
            .await
            .expect("run should complete")
            .into_output();

        // The echoed prompt still leads with system prompt, preamble
        // messages in configured order, then the original prompt.
        assert_eq!(
            output,
            "agent system override\n\n[System] sys note\n\n[User] user note\n\nbase prompt"
        );

        // The same run carried the override, proving prompt handling is
        // untouched while model settings change.
        let seen = captured
            .lock()
            .expect("captured settings should not be poisoned");
        assert_eq!(seen[0].temperature, Some(f64::from(0.9_f32)));
        assert_eq!(
            seen[0].top_p,
            Some(f64::from(0.8_f32)),
            "agent-configured top_p should be retained"
        );
    }

    /// Run-config hook that records each dispatch it observes and injects
    /// one preamble section, standing in for config-hooks-only
    /// registrations.
    struct RecordingConfigHook {
        fires: Arc<Mutex<Vec<String>>>,
    }

    impl RunConfigHook for RecordingConfigHook {
        fn configure<'a>(
            &'a self,
            ctx: &'a HookRunContext<'a>,
            config: &'a mut RunConfig,
        ) -> RunConfigHookFuture<'a> {
            Box::pin(async move {
                self.fires
                    .lock()
                    .expect("fires should not be poisoned")
                    .push(ctx.run_id.to_string());
                config.preamble_messages.push(PreambleMessage {
                    role: PreambleRole::User,
                    content: "config-only note".into(),
                });
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn run_applies_config_hooks_when_no_run_hooks_are_registered() {
        // A config-hooks-only registration leaves the run-hook chain
        // empty, so the dispatch gate must still route through the hook
        // machinery instead of taking the direct fast path.
        let fires = Arc::new(Mutex::new(Vec::new()));
        let (hooked, _captured) = hooked_agent_with_settings_capture(
            HookSet::builder()
                .run_config_hook(RecordingConfigHook {
                    fires: Arc::clone(&fires),
                })
                .build(),
        );

        let output = hooked
            .run("base prompt", ())
            .await
            .expect("run should complete")
            .into_output();

        assert_eq!(
            output, "[User] config-only note\n\nbase prompt",
            "the config hook's preamble section must reach the model request"
        );
        let fires = fires.lock().expect("fires should not be poisoned");
        assert_eq!(fires.len(), 1, "the config hook must fire exactly once");
        assert!(!fires[0].is_empty(), "the hook context must carry a run id");
    }

    /// Model whose every request fails, so the inner agent run surfaces a
    /// real `AgentRunError::Model` failure.
    struct FailingModel {
        profile: serdes_ai_models::ModelProfile,
    }

    impl FailingModel {
        fn new() -> Self {
            Self {
                profile: serdes_ai_models::ModelProfile::default(),
            }
        }
    }

    #[async_trait::async_trait]
    impl serdes_ai_models::Model for FailingModel {
        fn name(&self) -> &str {
            "failing-model"
        }

        fn system(&self) -> &str {
            "test"
        }

        fn profile(&self) -> &serdes_ai_models::ModelProfile {
            &self.profile
        }

        async fn request(
            &self,
            _messages: &[ModelRequest],
            _settings: &serdes_ai::core::ModelSettings,
            _params: &serdes_ai_models::ModelRequestParameters,
        ) -> Result<serdes_ai::core::ModelResponse, serdes_ai_models::ModelError> {
            Err(serdes_ai_models::ModelError::api("upstream exploded"))
        }

        async fn request_stream(
            &self,
            _messages: &[ModelRequest],
            _settings: &serdes_ai::core::ModelSettings,
            _params: &serdes_ai_models::ModelRequestParameters,
        ) -> Result<serdes_ai_models::StreamedResponse, serdes_ai_models::ModelError> {
            Err(serdes_ai_models::ModelError::api("upstream exploded"))
        }
    }

    /// Run hook that delegates straight to `original`, standing in for any
    /// observer hook that never interferes with the run.
    struct PassthroughRunHook;

    impl RunHook for PassthroughRunHook {
        fn hook<'a>(
            &'a self,
            ctx: &'a HookRunContext<'a>,
            _config: &'a RunConfig,
            original: RunOriginal<'a>,
        ) -> RunHookFuture<'a> {
            original.call(ctx)
        }
    }

    /// Run hook that skips `original` and fails on its own.
    struct FailingRunHook;

    impl RunHook for FailingRunHook {
        fn hook<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            _config: &'a RunConfig,
            _original: RunOriginal<'a>,
        ) -> RunHookFuture<'a> {
            Box::pin(async {
                Err(reloaded_code_core::ToolError::Execution(
                    "hook rejected the run".into(),
                ))
            })
        }
    }

    /// Run hook that observes the original run's failure and substitutes its
    /// own error instead of propagating it.
    struct SubstitutingRunHook;

    impl RunHook for SubstitutingRunHook {
        fn hook<'a>(
            &'a self,
            ctx: &'a HookRunContext<'a>,
            _config: &'a RunConfig,
            original: RunOriginal<'a>,
        ) -> RunHookFuture<'a> {
            Box::pin(async move {
                match original.call(ctx).await {
                    Err(_) => Err(reloaded_code_core::ToolError::Execution(
                        "policy veto".into(),
                    )),
                    ok => ok,
                }
            })
        }
    }

    /// Run-config hook that fails its configuration step.
    struct FailingConfigHook;

    impl RunConfigHook for FailingConfigHook {
        fn configure<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            _config: &'a mut RunConfig,
        ) -> RunConfigHookFuture<'a> {
            Box::pin(async {
                Err(reloaded_code_core::ToolError::Execution(
                    "config hook rejected the run".into(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn run_failure_keeps_original_error_variant_when_hook_propagates_it_untouched() {
        // The hook calls `original`, so the inner model failure flows
        // through the whole chain before reaching the caller.
        let hooks = HookSet::builder().run_hook(PassthroughRunHook).build();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(FailingModel::new());
        let hooked = context.build("caller").expect("build should succeed");

        let err = hooked
            .run("trigger the failure", ())
            .await
            .err()
            .expect("run should fail");

        assert!(
            matches!(
                err,
                serdes_ai::agent::AgentRunError::Model(serdes_ai_models::ModelError::Api { .. })
            ),
            "inner model failure should keep its variant, got: {err:?}"
        );
        assert!(
            !err.to_string().contains("run hook error"),
            "untouched inner failure must not be labeled as a hook error: {err}"
        );
    }

    #[tokio::test]
    async fn run_failure_keeps_original_error_variant_when_model_settings_overrides_are_present() {
        // The override routes the run through `run_with_options`; its
        // failures must keep the same untouched-projection handling as
        // plain runs.
        let hooks = HookSet::builder()
            .run_config_hook(OverridingConfigHook {
                temperature: Some(0.9),
                top_p: None,
            })
            .build();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(FailingModel::new());
        let hooked = context.build("caller").expect("build should succeed");

        let err = hooked
            .run("trigger the failure", ())
            .await
            .err()
            .expect("run should fail");

        assert!(
            matches!(
                err,
                serdes_ai::agent::AgentRunError::Model(serdes_ai_models::ModelError::Api { .. })
            ),
            "inner model failure should keep its variant, got: {err:?}"
        );
        assert!(
            !err.to_string().contains("run hook error"),
            "untouched inner failure must not be labeled as a hook error: {err}"
        );
    }

    #[tokio::test]
    async fn run_failure_is_labeled_hook_error_when_hook_returns_its_own_error() {
        // The hook skips `original` and fails on its own, so the model is
        // never invoked and the failure can only be hook-origin.
        let hooks = HookSet::builder().run_hook(FailingRunHook).build();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(crate::mock::MockModel::new("unused").with_text_response("unused"));
        let hooked = context.build("caller").expect("build should succeed");

        let err = hooked
            .run("trigger the hook failure", ())
            .await
            .err()
            .expect("run should fail");

        match err {
            serdes_ai::agent::AgentRunError::Other(source) => {
                let message = source.to_string();
                assert!(
                    message.contains("run hook error"),
                    "hook-origin failure should be labeled as such: {message}"
                );
                assert!(
                    message.contains("hook rejected the run"),
                    "dispatched hook error should be preserved: {message}"
                );
            }
            other => panic!("hook-substituted failure should surface as Other, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_failure_is_labeled_hook_error_when_hook_substitutes_its_own_error() {
        // The hook calls `original`, sees the model failure, then returns a
        // different error: the inner failure was observed but replaced, so
        // the surfaced failure is hook-origin, not the inner one.
        let hooks = HookSet::builder().run_hook(SubstitutingRunHook).build();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(FailingModel::new());
        let hooked = context.build("caller").expect("build should succeed");

        let err = hooked
            .run("trigger the failure", ())
            .await
            .err()
            .expect("run should fail");

        match err {
            serdes_ai::agent::AgentRunError::Other(source) => {
                let message = source.to_string();
                assert!(
                    message.contains("run hook error"),
                    "hook-substituted failure should be labeled hook-origin: {message}"
                );
                assert!(
                    message.contains("policy veto"),
                    "hook's substituted error should be preserved: {message}"
                );
            }
            other => {
                panic!("hook-substituted failure must not surface the inner error, got: {other:?}")
            }
        }
    }

    #[tokio::test]
    async fn run_failure_is_labeled_config_hook_error_when_config_hook_fails() {
        // The config hook fails before the run chain or the model starts,
        // so the model is never invoked and the failure can only be
        // config-hook-origin.
        let hooks = HookSet::builder()
            .run_config_hook(FailingConfigHook)
            .build();

        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(crate::mock::MockModel::new("unused").with_text_response("unused"));
        let hooked = context.build("caller").expect("build should succeed");

        let err = hooked
            .run("trigger the config hook failure", ())
            .await
            .err()
            .expect("run should fail");

        match err {
            serdes_ai::agent::AgentRunError::Other(source) => {
                let message = source.to_string();
                assert!(
                    message.contains("run config hook error"),
                    "config-hook failure should be labeled as such: {message}"
                );
                assert!(
                    message.contains("config hook rejected the run"),
                    "dispatched config-hook error should be preserved: {message}"
                );
            }
            other => panic!("config-hook failure should surface as Other, got: {other:?}"),
        }
    }
}
