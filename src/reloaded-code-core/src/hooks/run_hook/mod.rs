//! Run hook types -- intercept trait, config, output, and chain trampoline.

use crate::hooks::session::{EndReason, HookRunContext};
use crate::ToolError;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Result alias for run hook operations. Re-uses [ToolError].
pub type RunResult<T> = Result<T, ToolError>;

/// Boxed future returned by [`RunHook::hook`] and [`RunExecutor::execute`].
pub type RunHookFuture<'a> = Pin<Box<dyn Future<Output = RunResult<RunOutput>> + Send + 'a>>;

/// Preamble message injected before the user's prompt.
#[derive(Debug, Clone)]
pub struct PreambleMessage {
    /// Role of the preamble message.
    pub role: PreambleRole,
    /// Content of the preamble message.
    pub content: String,
}

/// Role for a preamble message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreambleRole {
    /// System-level instruction.
    System,
    /// User-level context.
    User,
}

/// Model-level settings that a RunHook can override.
#[derive(Default)]
pub struct ModelSettingsOverrides {
    /// Temperature override.
    pub temperature: Option<f32>,
    /// Top-p override.
    pub top_p: Option<f32>,
}

/// Mutable config a RunHook can change before calling original.
#[derive(Default)]
pub struct RunConfig {
    /// Override the agent's default system prompt.
    pub system_prompt: Option<String>,
    /// Preamble messages injected before the user prompt.
    pub preamble_messages: Vec<PreambleMessage>,
    /// Model settings overrides (temperature, top_p, etc.).
    pub model_settings_overrides: Option<ModelSettingsOverrides>,
}

/// Token usage for a completed run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunUsage {
    /// Tokens consumed in the prompt.
    pub prompt_tokens: u64,
    /// Tokens consumed in the completion.
    pub completion_tokens: u64,
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

/// Intercept hook for the full run lifecycle.
///
/// Code before `original` = inject preamble, override config.
/// Skip `original` = skip the run (return a synthetic `RunOutput`).
/// Code after = observe the run result.
///
/// `config` is owned (same as `ToolRequest` in `ToolHook`). Each hook
/// takes ownership, mutates, and passes to `original.call()`. The final
/// `RunExecutor` consumes it - strings move into the framework's run
/// options with zero clones.
pub trait RunHook: Send + Sync + 'static {
    /// Intercepts a run.
    ///
    /// # Errors
    /// Returns `ToolError` if the hook implementation or downstream executor fails.
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a>;
}

impl<F> RunHook for F
where
    F: for<'a> Fn(&'a HookRunContext<'a>, RunConfig, RunOriginal<'a>) -> RunHookFuture<'a>
        + Send
        + Sync
        + 'static,
{
    #[inline]
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        self(ctx, config, original)
    }
}

/// Managed trampoline to the next hook or real run executor.
///
/// `RunOriginal` is consumed by [`call`](Self::call), so normal hooks call
/// the continuation once.
pub struct RunOriginal<'a> {
    chain: &'a [Arc<dyn RunHook>],
    index: usize,
    real_run: &'a dyn RunExecutor,
}

impl<'a> RunOriginal<'a> {
    /// Creates a trampoline over the provided hook chain and real run executor.
    #[inline]
    #[must_use]
    pub fn new(chain: &'a [Arc<dyn RunHook>], real_run: &'a dyn RunExecutor) -> Self {
        Self {
            chain,
            index: 0,
            real_run,
        }
    }

    /// Calls the next hook, or the real run executor when no hooks remain.
    ///
    /// # Errors
    /// Returns `ToolError` if a downstream hook or the real executor returns an error.
    #[inline]
    pub fn call(self, ctx: &'a HookRunContext<'a>, config: RunConfig) -> RunHookFuture<'a> {
        if let Some(hook) = self.chain.get(self.index) {
            hook.hook(
                ctx,
                config,
                Self {
                    chain: self.chain,
                    index: self.index + 1,
                    real_run: self.real_run,
                },
            )
        } else {
            self.real_run.execute(ctx, config)
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

/// Final callable used when the hook chain reaches the real run executor.
pub trait RunExecutor: Send + Sync {
    /// Executes the real run.
    ///
    /// # Errors
    /// Returns `ToolError` if the real run executor encounters an error.
    fn execute<'a>(&'a self, ctx: &'a HookRunContext<'a>, config: RunConfig) -> RunHookFuture<'a>;
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
                _config: RunConfig,
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
        let hook: Arc<dyn RunHook> = Arc::new(MockHook);
        let output = hook
            .hook(&ctx, RunConfig::default(), RunOriginal::new(&[], &RealRun))
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
        let original = RunOriginal::new(&[], &RealRun);
        let output = original.call(&ctx, RunConfig::default()).await.unwrap();
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
        let original = RunOriginal::new(&chain, &RealRun);
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
