//! Run hook types: intercept trait, config, output, and chain trampoline.
//!
//! # What a run is
//!
//! One run = one `agent.run()` call, start to finish.
//! The framework is headless: no persistent conversation, no branching,
//! no multi-session switching. One API call starts exactly one run.
//!
//! A run holds N steps. One step = one LLM request plus the tool calls
//! it triggers. A run with no tool calls is a single step.
//!
//! # The run boundary
//!
//! A run hook wraps that whole boundary. Code before `original` runs
//! before the first step and observes the final config read-only;
//! code after sees the finished [`RunOutput`]. Skipping `original`
//! skips the run and returns a synthetic result instead.
//!
//! # Hook ownership
//!
//! [`RunConfigHook`] amends config before the run. [`RunHook`]
//! controls run lifecycle on `run()` only. [`RunEventHook`] owns
//! streamed events.
//!
//! # Run identity
//!
//! Each run carries a `run_id` (see [`HookRunContext`]). Tool hooks
//! fire inside a run, once per tool call, under the same `run_id`.
//!
//! Next: see [`ToolHook`] for the innermost intercept point.
//!
//! [`RunEventHook`]: crate::hooks::RunEventHook
//! [`ToolHook`]: crate::hooks::ToolHook

use crate::ToolError;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Boxed future returned by [`RunConfigHook::configure`].
pub type RunConfigHookFuture<'a> = Pin<Box<dyn Future<Output = RunResult<()>> + Send + 'a>>;

/// Boxed future returned by [`RunHook::hook`] and [`RunExecutor::execute`].
pub type RunHookFuture<'a> = Pin<Box<dyn Future<Output = RunResult<RunOutput>> + Send + 'a>>;

/// Managed trampoline to the next hook or real run executor.
///
/// `RunOriginal` is consumed by [`call`], so normal hooks call
/// the continuation once. It carries a shared view of the final
/// [`RunConfig`]; the chain end hands the executor an owned clone
/// of it (the only copy on the config flow).
///
/// [`call`]: Self::call
pub struct RunOriginal<'a> {
    chain: &'a [Arc<dyn RunHook>],
    index: usize,
    real_run: &'a dyn RunExecutor,
    config: &'a RunConfig,
}

/// Compact event callback. Name preserved - compact is its own concept, distinct from "run".
pub type SessionCompactFn = for<'a> fn(&'a HookRunContext<'a>);

/// Context given to hook run lifecycle events.
#[derive(Debug)]
pub struct HookRunContext<'a> {
    /// Name of the agent running the hook.
    pub agent_name: &'a str,
    /// Unique identifier for the current run.
    pub run_id: &'a str,
    /// Name of the model being used for this run.
    pub model_name: &'a str,
}

/// Run config: system prompt, preamble messages, model settings.
///
/// A [`RunConfigHook`] amends this config before the run; a [`RunHook`]
/// observes the final config read-only. `Clone` exists for the
/// chain-end hand-off: the trampoline clones once per run; every
/// other path moves the config.
#[derive(Default, Clone)]
pub struct RunConfig {
    /// Override the agent's default system prompt.
    pub system_prompt: Option<String>,
    /// Preamble messages injected before the user prompt.
    pub preamble_messages: Vec<PreambleMessage>,
    /// Model settings overrides (temperature, top_p, etc.).
    pub model_settings_overrides: Option<ModelSettingsOverrides>,
}

/// Result of a completed run. Framework-agnostic distillation of the agent output.
#[derive(Debug)]
pub struct RunOutput {
    /// The text output from the run.
    pub content: String,
    /// Why the run ended.
    pub reason: EndReason,
    /// Token usage consumed during the run.
    pub usage: RunUsage,
}

/// Result alias for run hook operations. Re-uses [`ToolError`].
pub type RunResult<T> = Result<T, ToolError>;

/// Why a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Run completed normally.
    Completed,
    /// Run was stopped externally.
    Stopped,
    /// Run failed (LLM error, length limit, content filter).
    Failed,
}

/// Model-level settings that a run config hook can override.
///
/// Derives `Clone` together with [`RunConfig`].
#[derive(Default, Clone)]
pub struct ModelSettingsOverrides {
    /// Temperature override.
    pub temperature: Option<f32>,
    /// Top-p override.
    pub top_p: Option<f32>,
}

/// Preamble message injected before the user's prompt.
#[derive(Debug, Clone)]
pub struct PreambleMessage {
    /// Role of the preamble message.
    pub role: PreambleRole,
    /// Content of the preamble message.
    pub content: String,
}

/// Token usage for a completed run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunUsage {
    /// Tokens consumed in the prompt.
    pub prompt_tokens: u64,
    /// Tokens consumed in the completion.
    pub completion_tokens: u64,
}

/// Role for a preamble message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreambleRole {
    /// System-level instruction.
    System,
    /// User-level context.
    User,
}

/// Hook that amends a run's config before the run starts.
///
/// `configure` mutates the [`RunConfig`] in place. Hooks run in
/// registration order; each hook sees every earlier hook's
/// mutations. Config hooks fire on both run paths (`run()` and
/// `run_stream()`); on `run()` they run before the run hook chain.
///
/// # Remarks
///
/// `configure` is async so a hook can fetch remote resources (prompt
/// templates, feature flags). The future is boxed once per hook per
/// run.
pub trait RunConfigHook: Send + Sync + 'static {
    /// Amends the run config in place.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the hook fails. The chain stops at the
    /// first error and the run does not start.
    ///
    /// [`ToolError`]: crate::ToolError
    fn configure<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: &'a mut RunConfig,
    ) -> RunConfigHookFuture<'a>;
}

/// Final callable used when the hook chain reaches the real run executor.
pub trait RunExecutor: Send + Sync {
    /// Executes the real run with the final, owned [`RunConfig`].
    ///
    /// # Errors
    /// Returns `ToolError` if the real run executor encounters an error.
    fn execute<'a>(&'a self, ctx: &'a HookRunContext<'a>, config: RunConfig) -> RunHookFuture<'a>;
}

/// Intercept hook for the full run lifecycle.
///
/// Code before `original` = observe the run before it starts.
/// Skip `original` = skip the run (return a synthetic `RunOutput`).
/// Code after = observe the run result.
///
/// `config` is a read-only view of the final [`RunConfig`]. To
/// change it, register a [`RunConfigHook`].
///
/// # Remarks
///
/// Fires only on the `run()` path - never on streaming runs.
/// Use [`RunEventHook`] to intercept streamed events.
///
/// [`RunEventHook`]: crate::hooks::RunEventHook
pub trait RunHook: Send + Sync + 'static {
    /// Intercepts a run.
    ///
    /// # Errors
    /// Returns `ToolError` if the hook implementation or downstream executor fails.
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: &'a RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a>;
}

impl<'a> RunOriginal<'a> {
    /// Creates a trampoline over the provided hook chain, final
    /// config, and real run executor.
    #[inline]
    #[must_use]
    pub fn new(
        chain: &'a [Arc<dyn RunHook>],
        real_run: &'a dyn RunExecutor,
        config: &'a RunConfig,
    ) -> Self {
        Self {
            chain,
            index: 0,
            real_run,
            config,
        }
    }

    /// Calls the next hook, or the real run executor when no hooks
    /// remain.
    ///
    /// The chain end hands the executor an owned clone of the final
    /// config: the single copy on the config flow when run hooks are
    /// registered.
    ///
    /// # Errors
    /// Returns `ToolError` if a downstream hook or the real executor returns an error.
    #[inline]
    pub fn call(self, ctx: &'a HookRunContext<'a>) -> RunHookFuture<'a> {
        if let Some(hook) = self.chain.get(self.index) {
            hook.hook(
                ctx,
                self.config,
                Self {
                    chain: self.chain,
                    index: self.index + 1,
                    real_run: self.real_run,
                    config: self.config,
                },
            )
        } else {
            self.real_run.execute(ctx, self.config.clone())
        }
    }
}

impl fmt::Debug for RunOriginal<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunOriginal")
            .field("chain_len", &self.chain.len())
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl<F> RunHook for F
where
    F: for<'a> Fn(&'a HookRunContext<'a>, &'a RunConfig, RunOriginal<'a>) -> RunHookFuture<'a>
        + Send
        + Sync
        + 'static,
{
    #[inline]
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: &'a RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        self(ctx, config, original)
    }
}

impl<F> RunExecutor for F
where
    F: for<'a> Fn(&'a HookRunContext<'a>, RunConfig) -> RunHookFuture<'a> + Send + Sync,
{
    #[inline]
    fn execute<'a>(&'a self, ctx: &'a HookRunContext<'a>, config: RunConfig) -> RunHookFuture<'a> {
        self(ctx, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_config_populated_holds_values() {
        let config = RunConfig {
            system_prompt: Some("sys".into()),
            preamble_messages: vec![PreambleMessage {
                role: PreambleRole::User,
                content: "ctx".into(),
            }],
            model_settings_overrides: Some(ModelSettingsOverrides {
                temperature: Some(0.5),
                top_p: Some(0.9),
            }),
        };
        assert_eq!(config.system_prompt.as_deref(), Some("sys"));
        assert_eq!(config.preamble_messages.len(), 1);
        assert_eq!(
            config.model_settings_overrides.unwrap().temperature,
            Some(0.5)
        );
    }

    #[tokio::test]
    async fn run_hook_closure_impl() {
        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "real".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        struct MockHook;
        impl RunHook for MockHook {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                _original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "mock".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let ctx = HookRunContext {
            agent_name: "test",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let config = RunConfig::default();
        let hook: Arc<dyn RunHook> = Arc::new(MockHook);
        let output = hook
            .hook(&ctx, &config, RunOriginal::new(&[], &RealRun, &config))
            .await
            .unwrap();
        assert_eq!(output.content, "mock");
    }

    #[tokio::test]
    async fn run_original_calls_real_executor_when_chain_empty() {
        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "real".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let ctx = HookRunContext {
            agent_name: "test",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let config = RunConfig::default();
        let original = RunOriginal::new(&[], &RealRun, &config);
        let output = original.call(&ctx).await.unwrap();
        assert_eq!(output.content, "real");
    }

    #[tokio::test]
    async fn run_original_debug_format() {
        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }
        let chain: Vec<Arc<dyn RunHook>> = vec![];
        let config = RunConfig::default();
        let original = RunOriginal::new(&chain, &RealRun, &config);
        let debug = format!("{:?}", original);
        assert!(debug.contains("RunOriginal"));
        assert!(debug.contains("chain_len"));
    }

    #[tokio::test]
    async fn run_executor_fn_impl() {
        struct FnExecutor;
        impl RunExecutor for FnExecutor {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "from-fn".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let ctx = HookRunContext {
            agent_name: "test",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let output = FnExecutor
            .execute(&ctx, RunConfig::default())
            .await
            .unwrap();
        assert_eq!(output.content, "from-fn");
    }
}
