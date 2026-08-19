//! HookSet - container and dispatch for all registered hooks.

use crate::hooks::{
    CompactExecutor, CompactHook, CompactHookFuture, CompactMessage, CompactOriginal,
    HookRunContext, RunConfig, RunConfigHook, RunEvent, RunEventContext, RunEventHook,
    RunEventHookResult, RunExecutor, RunHook, RunHookFuture, RunOriginal, RunResult,
    ToolCallContext, ToolExecutor, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest,
};
use std::fmt;
use std::sync::Arc;

/// All registered hooks, stored per point.
#[derive(Clone, Default)]
pub struct HookSet {
    pub(super) tool_hooks: Vec<Arc<dyn ToolHook>>,
    pub(super) run_config_hooks: Vec<Arc<dyn RunConfigHook>>,
    pub(super) run_hooks: Vec<Arc<dyn RunHook>>,
    pub(super) run_event_hooks: Vec<Arc<dyn RunEventHook>>,
    pub(super) compact_hooks: Vec<Arc<dyn CompactHook>>,
}

impl HookSet {
    /// Returns `true` if no hooks are registered at any point.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tool_hooks.is_empty()
            && self.run_config_hooks.is_empty()
            && self.run_hooks.is_empty()
            && self.run_event_hooks.is_empty()
            && self.compact_hooks.is_empty()
    }

    /// Returns `true` if no tool hooks are registered.
    #[inline]
    #[must_use]
    pub fn tool_hooks_is_empty(&self) -> bool {
        self.tool_hooks.is_empty()
    }

    /// Returns `true` if no run-config hooks are registered.
    #[inline]
    #[must_use]
    pub fn run_config_hooks_is_empty(&self) -> bool {
        self.run_config_hooks.is_empty()
    }

    /// Returns `true` if no run hooks are registered.
    #[inline]
    #[must_use]
    pub fn run_hooks_is_empty(&self) -> bool {
        self.run_hooks.is_empty()
    }

    /// Returns `true` if no run-event hooks are registered.
    #[inline]
    #[must_use]
    pub fn run_event_hooks_is_empty(&self) -> bool {
        self.run_event_hooks.is_empty()
    }

    /// Returns `true` if no compact hooks are registered.
    #[inline]
    #[must_use]
    pub fn compact_hooks_is_empty(&self) -> bool {
        self.compact_hooks.is_empty()
    }

    /// Returns registered tool hooks in dispatch order.
    #[inline]
    #[must_use]
    pub fn tool_hooks(&self) -> &[Arc<dyn ToolHook>] {
        &self.tool_hooks
    }

    /// Returns registered run-config hooks in dispatch order.
    #[inline]
    #[must_use]
    pub fn run_config_hooks(&self) -> &[Arc<dyn RunConfigHook>] {
        &self.run_config_hooks
    }

    /// Returns registered run hooks in dispatch order.
    #[inline]
    #[must_use]
    pub fn run_hooks(&self) -> &[Arc<dyn RunHook>] {
        &self.run_hooks
    }

    /// Returns a new builder for constructing a `HookSet`.
    #[inline]
    #[must_use]
    pub fn builder() -> crate::hooks::builder::HookSetBuilder {
        crate::hooks::builder::HookSetBuilder::new()
    }

    /// Dispatches a tool call through the hook chain.
    ///
    /// If no tool hooks are registered, this calls the real tool directly.
    #[inline]
    pub fn dispatch_tool<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        real_tool: &'a dyn ToolExecutor,
    ) -> ToolHookFuture<'a> {
        if self.tool_hooks.is_empty() {
            return real_tool.execute(ctx, req);
        }
        ToolOriginal::new(&self.tool_hooks, real_tool).call(ctx, req)
    }

    /// Applies the run-config hook chain to `config`.
    ///
    /// Hooks run in registration order, each mutating the same
    /// [`RunConfig`]. If no run-config hooks are registered, `config`
    /// is returned unchanged without entering the chain.
    ///
    /// # Errors
    /// Returns [`ToolError`] if any hook in the chain returns an error;
    /// dispatch stops at the first error.
    ///
    /// [`ToolError`]: crate::ToolError
    #[inline]
    pub async fn dispatch_run_config(
        &self,
        ctx: &HookRunContext<'_>,
        mut config: RunConfig,
    ) -> RunResult<RunConfig> {
        for hook in &self.run_config_hooks {
            hook.configure(ctx, &mut config).await?;
        }
        Ok(config)
    }

    /// Dispatches a run through the config and run hook chains.
    ///
    /// Config hooks run first, amending `config`. The run chain then
    /// runs with a shared view of the final config, which the executor
    /// receives as an owned clone. Empty chains are skipped.
    ///
    /// # Errors
    /// Returns [`ToolError`] if a config hook, any run hook in the
    /// chain, or the executor returns an error. A config-hook error
    /// stops dispatch before the run chain or executor starts.
    ///
    /// [`ToolError`]: crate::ToolError
    #[inline]
    pub fn dispatch_run<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: RunConfig,
        real_run: &'a dyn RunExecutor,
    ) -> RunHookFuture<'a> {
        if self.run_hooks.is_empty() && self.run_config_hooks.is_empty() {
            return real_run.execute(ctx, config);
        }
        Box::pin(async move {
            let final_config = self.dispatch_run_config(ctx, config).await?;
            if self.run_hooks.is_empty() {
                real_run.execute(ctx, final_config).await
            } else {
                RunOriginal::new(&self.run_hooks, real_run, &final_config)
                    .call(ctx)
                    .await
            }
        })
    }

    /// Dispatches one streamed run event through the run-event hook chain.
    ///
    /// Hooks apply in registration order; each hook receives the
    /// previous hook's output event. A suppression from any hook ends
    /// the chain for that event. If no run-event hooks are registered,
    /// the event is returned unchanged without entering the chain.
    ///
    /// # Errors
    /// Returns [`ToolError`] if any hook in the chain returns an error;
    /// dispatch stops at the first error.
    ///
    /// [`ToolError`]: crate::ToolError
    #[inline]
    pub fn dispatch_run_event(
        &self,
        ctx: &RunEventContext<'_>,
        mut event: RunEvent,
    ) -> RunEventHookResult {
        if self.run_event_hooks.is_empty() {
            return Ok(Some(event));
        }
        for hook in &self.run_event_hooks {
            match hook.hook(ctx, event)? {
                Some(next) => event = next,
                None => return Ok(None),
            }
        }
        Ok(Some(event))
    }

    /// Dispatches one compaction attempt through the compact hook chain.
    ///
    /// Hooks run in registration order over `history`; the default
    /// compaction runs at the chain end with the mutated history. If
    /// no compact hooks are registered, the default compaction runs
    /// directly without entering the chain.
    ///
    /// Returns the chain's outcome together with the history to
    /// apply: on [`CompactOutcome::Compacted`] apply the history; on
    /// [`CompactOutcome::Cancelled`] leave the run's history
    /// unchanged.
    ///
    /// # Errors
    /// Returns [`ToolError`] if any hook in the chain or the default
    /// compaction returns an error; dispatch stops at the first
    /// error.
    ///
    /// [`ToolError`]: crate::ToolError
    /// [`CompactOutcome::Compacted`]: crate::hooks::CompactOutcome::Compacted
    /// [`CompactOutcome::Cancelled`]: crate::hooks::CompactOutcome::Cancelled
    #[inline]
    pub fn dispatch_compact<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        history: Vec<CompactMessage>,
        real_compact: &'a dyn CompactExecutor,
    ) -> CompactHookFuture<'a> {
        if self.compact_hooks.is_empty() {
            real_compact.execute(ctx, history)
        } else {
            CompactOriginal::new(&self.compact_hooks, real_compact).call(ctx, history)
        }
    }
}

impl fmt::Debug for HookSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookSet")
            .field("tool_hooks", &self.tool_hooks.len())
            .field("run_config_hooks", &self.run_config_hooks.len())
            .field("run_hooks", &self.run_hooks.len())
            .field("run_event_hooks", &self.run_event_hooks.len())
            .field("compact_hooks", &self.compact_hooks.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::compact_hook::{CompactOutcome, CompactResult};
    use crate::hooks::run_event::{
        RunEvent, RunEventContext, RunEventHook, RunEventHookResult, RunMessageRole,
    };
    use crate::hooks::run_hook::{
        EndReason, ModelSettingsOverrides, PreambleMessage, PreambleRole, RunConfig, RunConfigHook,
        RunConfigHookFuture, RunExecutor, RunHook, RunHookFuture, RunOriginal, RunOutput, RunUsage,
    };
    use crate::{ToolError, ToolOutput};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn ready(output: impl Into<ToolOutput>) -> ToolHookFuture<'static> {
        let output = output.into();
        Box::pin(async move { Ok(output) })
    }

    /// Wraps a config mutation closure as a hook that always succeeds.
    fn mutate_hook<F>(mutate: F) -> impl RunConfigHook
    where
        F: Fn(&mut RunConfig) + Send + Sync + 'static,
    {
        struct Mutate<F>(F);
        impl<F> RunConfigHook for Mutate<F>
        where
            F: Fn(&mut RunConfig) + Send + Sync + 'static,
        {
            fn configure<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                config: &'a mut RunConfig,
            ) -> RunConfigHookFuture<'a> {
                Box::pin(async move {
                    (self.0)(config);
                    Ok(())
                })
            }
        }
        Mutate(mutate)
    }

    /// Builds a hook whose configure always fails validation.
    fn fail_hook(message: &'static str) -> impl RunConfigHook {
        struct Fail(&'static str);
        impl RunConfigHook for Fail {
            fn configure<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a mut RunConfig,
            ) -> RunConfigHookFuture<'a> {
                Box::pin(async { Err(ToolError::validation(self.0)) })
            }
        }
        Fail(message)
    }

    #[test]
    fn hook_set_default_is_empty() {
        let hooks = HookSet::default();
        assert!(hooks.is_empty());
        assert!(hooks.tool_hooks_is_empty());
        assert!(hooks.run_hooks_is_empty());
    }

    #[test]
    fn hook_set_with_run_hooks_is_not_empty() {
        struct NoopRun;
        impl RunHook for NoopRun {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                original.call(ctx)
            }
        }
        let hooks = HookSet::builder().run_hook(NoopRun).build();
        assert!(!hooks.is_empty());
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[tokio::test]
    async fn dispatch_tool_empty_calls_real_tool_directly() {
        struct RealTool;

        impl ToolExecutor for RealTool {
            fn execute<'a>(
                &'a self,
                _ctx: &'a ToolCallContext<'a>,
                req: ToolRequest,
            ) -> ToolHookFuture<'a> {
                let content = req.args["value"].as_str().unwrap().to_string();
                Box::pin(async move { Ok(ToolOutput::new(content)) })
            }
        }

        let hooks = HookSet::default();
        let ctx = ToolCallContext {
            tool_name: "echo",
            agent_name: "coder",
            run_id: "r1",
        };
        let output = hooks
            .dispatch_tool(&ctx, ToolRequest::new(json!({"value": "ok"})), &RealTool)
            .await
            .unwrap();

        assert_eq!(output.content, "ok");
    }

    #[tokio::test]
    async fn dispatch_tool_hooks_wrap_real_tool() {
        struct Prefix;
        struct Suffix;
        struct RealTool;

        impl ToolHook for Prefix {
            fn hook<'a>(
                &'a self,
                ctx: &'a ToolCallContext<'a>,
                mut req: ToolRequest,
                original: ToolOriginal<'a>,
            ) -> ToolHookFuture<'a> {
                Box::pin(async move {
                    req.args["value"] =
                        json!(format!("pre-{}", req.args["value"].as_str().unwrap()));
                    let mut output = original.call(ctx, req).await?;
                    output.content.push_str("-post");
                    Ok(output)
                })
            }
        }

        impl ToolHook for Suffix {
            fn hook<'a>(
                &'a self,
                ctx: &'a ToolCallContext<'a>,
                mut req: ToolRequest,
                original: ToolOriginal<'a>,
            ) -> ToolHookFuture<'a> {
                Box::pin(async move {
                    req.args["value"] =
                        json!(format!("{}-inner", req.args["value"].as_str().unwrap()));
                    let mut output = original.call(ctx, req).await?;
                    output.content.push_str("-innerpost");
                    Ok(output)
                })
            }
        }

        impl ToolExecutor for RealTool {
            fn execute<'a>(
                &'a self,
                _ctx: &'a ToolCallContext<'a>,
                req: ToolRequest,
            ) -> ToolHookFuture<'a> {
                let content = req.args["value"].as_str().unwrap().to_string();
                Box::pin(async move { Ok(ToolOutput::new(content)) })
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .tool_hook(Prefix)
            .tool_hook(Suffix)
            .build();
        let ctx = ToolCallContext {
            tool_name: "echo",
            agent_name: "coder",
            run_id: "r1",
        };
        let output = hooks
            .dispatch_tool(&ctx, ToolRequest::new(json!({"value": "x"})), &RealTool)
            .await
            .unwrap();

        assert_eq!(output.content, "pre-x-inner-innerpost-post");
    }

    #[tokio::test]
    async fn dispatch_tool_hook_can_block_without_calling_original() {
        struct Block;
        struct RealTool;

        impl ToolHook for Block {
            fn hook<'a>(
                &'a self,
                _ctx: &'a ToolCallContext<'a>,
                _req: ToolRequest,
                _original: ToolOriginal<'a>,
            ) -> ToolHookFuture<'a> {
                Box::pin(async { Ok(ToolOutput::new("blocked")) })
            }
        }

        impl ToolExecutor for RealTool {
            fn execute<'a>(
                &'a self,
                _ctx: &'a ToolCallContext<'a>,
                _req: ToolRequest,
            ) -> ToolHookFuture<'a> {
                ready("should not run")
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .tool_hook(Block)
            .build();
        let ctx = ToolCallContext {
            tool_name: "bash",
            agent_name: "coder",
            run_id: "r1",
        };
        let output = hooks
            .dispatch_tool(&ctx, ToolRequest::new(json!({})), &RealTool)
            .await
            .unwrap();

        assert_eq!(output.content, "blocked");
    }

    // --- Run config dispatch tests ---------------------------------------------

    fn run_ctx() -> HookRunContext<'static> {
        HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        }
    }

    #[tokio::test]
    async fn dispatch_run_config_applies_hooks_in_registration_order() {
        let hooks = HookSet::builder()
            .run_config_hook(mutate_hook(|config| {
                config.system_prompt = Some("base".into());
            }))
            .run_config_hook(mutate_hook(|config| {
                let tagged = format!("{}-tagged", config.system_prompt.take().unwrap_or_default());
                config.system_prompt = Some(tagged);
            }))
            .build();
        let config = hooks
            .dispatch_run_config(&run_ctx(), RunConfig::default())
            .await
            .unwrap();

        // "base-tagged" proves the second hook saw the first hook's
        // mutation, not its own starting value.
        assert_eq!(config.system_prompt.as_deref(), Some("base-tagged"));
    }

    #[tokio::test]
    async fn dispatch_run_config_accumulates_mutations_across_hooks() {
        let mut input = RunConfig::default();
        input.preamble_messages.push(PreambleMessage {
            role: PreambleRole::System,
            content: "seeded".into(),
        });

        let hooks = HookSet::builder()
            .run_config_hook(mutate_hook(|config| {
                config.system_prompt = Some("sys".into());
            }))
            .run_config_hook(mutate_hook(|config| {
                config.preamble_messages.push(PreambleMessage {
                    role: PreambleRole::User,
                    content: "ctx".into(),
                });
                config.model_settings_overrides = Some(ModelSettingsOverrides {
                    temperature: Some(0.2),
                    top_p: Some(0.9),
                });
            }))
            .build();
        let config = hooks.dispatch_run_config(&run_ctx(), input).await.unwrap();

        // The caller's seeded preamble survives and every hook's field
        // writes accumulate: the config is threaded through, not replaced.
        assert_eq!(config.system_prompt.as_deref(), Some("sys"));
        assert_eq!(config.preamble_messages.len(), 2);
        let overrides = config.model_settings_overrides.unwrap();
        assert_eq!(overrides.temperature, Some(0.2));
        assert_eq!(overrides.top_p, Some(0.9));
    }

    #[tokio::test]
    async fn dispatch_run_config_stops_at_first_error() {
        struct MustNotRun;

        impl RunConfigHook for MustNotRun {
            fn configure<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a mut RunConfig,
            ) -> RunConfigHookFuture<'a> {
                panic!("later hooks must not run after a hook error");
            }
        }

        let hooks = HookSet::builder()
            .run_config_hook(fail_hook("config rejected the run"))
            .run_config_hook(MustNotRun)
            .build();
        let result = hooks
            .dispatch_run_config(&run_ctx(), RunConfig::default())
            .await;
        assert!(matches!(result, Err(ToolError::Validation { .. })));
    }

    #[tokio::test]
    async fn dispatch_run_config_empty_chain_returns_config_unchanged() {
        struct Passthrough;
        impl RunEventHook for Passthrough {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                Ok(Some(event))
            }
        }

        let input = RunConfig {
            system_prompt: Some("sys".into()),
            preamble_messages: vec![PreambleMessage {
                role: PreambleRole::System,
                content: "ctx".into(),
            }],
            ..RunConfig::default()
        };

        // Other hook chains registered, config chain empty: the config
        // bypasses the chain untouched.
        let hooks = HookSet::builder().run_event_hook(Passthrough).build();
        assert!(!hooks.is_empty());
        assert!(hooks.run_config_hooks_is_empty());

        let config = hooks.dispatch_run_config(&run_ctx(), input).await.unwrap();
        assert_eq!(config.system_prompt.as_deref(), Some("sys"));
        assert_eq!(config.preamble_messages.len(), 1);
    }

    #[test]
    fn hook_set_with_run_config_hooks_is_not_empty() {
        let hooks = HookSet::builder()
            .run_config_hook(mutate_hook(|_| {}))
            .build();
        assert!(!hooks.is_empty());
        assert!(!hooks.run_config_hooks_is_empty());
        assert_eq!(hooks.run_config_hooks().len(), 1);
    }

    #[test]
    // Pins manual Debug: counts only, never hook contents (traits lack Debug).
    fn hook_set_debug_includes_run_config_hooks_count() {
        let hooks = HookSet::builder()
            .run_config_hook(mutate_hook(|_| {}))
            .build();
        let debug = format!("{hooks:?}");
        assert!(debug.contains("run_config_hooks: 1"));
    }

    // --- Run dispatch tests ----------------------------------------------------

    #[tokio::test]
    async fn dispatch_run_empty_calls_real_run_directly() {
        struct RealRun;

        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                config: RunConfig,
            ) -> RunHookFuture<'a> {
                let content = config.system_prompt.unwrap_or_else(|| "default".into());
                Box::pin(async move {
                    Ok(RunOutput {
                        content,
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let hooks = HookSet::default();
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let output = hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(output.content, "default");
    }

    #[tokio::test]
    async fn dispatch_run_hooks_wrap_real_run() {
        struct Wrap;
        impl RunHook for Wrap {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async move {
                    // The run hook sees the config-hook-amended final
                    // config: the same values the executor receives.
                    let seen = config.system_prompt.clone();
                    let mut output = original.call(ctx).await?;
                    output
                        .content
                        .push_str(&format!("-saw:{}-post", seen.unwrap_or_default()));
                    Ok(output)
                })
            }
        }

        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                config: RunConfig,
            ) -> RunHookFuture<'a> {
                let prompt = config.system_prompt.unwrap_or_else(|| "default".into());
                let preamble = config
                    .preamble_messages
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("+");
                Box::pin(async move {
                    Ok(RunOutput {
                        content: format!("{prompt}|{preamble}"),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_config_hook(mutate_hook(|config| {
                config.system_prompt = Some("overridden".into());
                config.preamble_messages.push(PreambleMessage {
                    role: PreambleRole::User,
                    content: "ctx".into(),
                });
            }))
            .run_hook(Wrap)
            .build();
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        // Seed a preamble no hook writes: the executor's owned config
        // must carry both the seed and the hook-amended fields.
        let mut input = RunConfig::default();
        input.preamble_messages.push(PreambleMessage {
            role: PreambleRole::System,
            content: "seeded".into(),
        });
        let output = hooks.dispatch_run(&ctx, input, &RealRun).await.unwrap();

        assert_eq!(output.content, "overridden|seeded+ctx-saw:overridden-post");
        assert_eq!(output.reason, EndReason::Completed);
    }

    #[tokio::test]
    async fn dispatch_run_config_hooks_only_feed_executor() {
        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                config: RunConfig,
            ) -> RunHookFuture<'a> {
                let content = config.system_prompt.unwrap_or_else(|| "default".into());
                Box::pin(async move {
                    Ok(RunOutput {
                        content,
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_config_hook(mutate_hook(|config| {
                config.system_prompt = Some("cfg-only".into());
            }))
            .build();
        assert!(hooks.run_hooks_is_empty());
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let output = hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(output.content, "cfg-only");
    }

    #[tokio::test]
    async fn dispatch_run_config_hook_error_aborts_before_run_chain() {
        struct MustNotRun;
        impl RunHook for MustNotRun {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                _original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                panic!("run hooks must not run after a config hook error");
            }
        }

        struct PanicRun;
        impl RunExecutor for PanicRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                panic!("executor must not run after a config hook error");
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_config_hook(fail_hook("config rejected the run"))
            .run_hook(MustNotRun)
            .build();
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let result = hooks
            .dispatch_run(&ctx, RunConfig::default(), &PanicRun)
            .await;
        assert!(matches!(result, Err(ToolError::Validation { .. })));
    }

    #[tokio::test]
    async fn dispatch_run_hook_error_stops_the_chain() {
        struct FailingHook;
        impl RunHook for FailingHook {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                _original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async { Err(ToolError::validation("hook rejected the run")) })
            }
        }

        struct MustNotRun;
        impl RunHook for MustNotRun {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                _original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                panic!("later hooks must not run after a hook error");
            }
        }

        struct PanicRun;
        impl RunExecutor for PanicRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                panic!("executor must not run after a hook error");
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_hook(FailingHook)
            .run_hook(MustNotRun)
            .build();
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let result = hooks
            .dispatch_run(&ctx, RunConfig::default(), &PanicRun)
            .await;
        assert!(matches!(result, Err(ToolError::Validation { .. })));
    }

    #[tokio::test]
    async fn dispatch_run_hook_can_skip_without_calling_original() {
        struct Skip;
        struct RealRun;

        impl RunHook for Skip {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                _original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "skipped".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "should not run".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_hook(Skip)
            .build();
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };
        let output = hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(output.content, "skipped");
    }

    #[tokio::test]
    async fn dispatch_run_two_hooks_unwind_order() {
        use std::sync::Mutex;
        static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

        struct First;
        struct Second;

        impl RunHook for First {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                LOG.lock().unwrap().push("first-before".into());
                Box::pin(async move {
                    let output = original.call(ctx).await?;
                    LOG.lock().unwrap().push("first-after".into());
                    Ok(output)
                })
            }
        }

        impl RunHook for Second {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                LOG.lock().unwrap().push("second-before".into());
                Box::pin(async move {
                    let output = original.call(ctx).await?;
                    LOG.lock().unwrap().push("second-after".into());
                    Ok(output)
                })
            }
        }

        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "ok".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        LOG.lock().unwrap().clear();
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_hook(First)
            .run_hook(Second)
            .build();
        let ctx = HookRunContext {
            agent_name: "t",
            run_id: "r1",
            model_name: "m",
        };
        hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        let log = LOG.lock().unwrap();
        assert_eq!(
            *log,
            vec![
                "first-before".to_string(),
                "second-before".to_string(),
                "second-after".to_string(),
                "first-after".to_string(),
            ]
        );
    }

    // --- Run event dispatch tests ---------------------------------------------

    fn event_ctx() -> RunEventContext<'static> {
        RunEventContext {
            agent_name: "coder",
            model_name: "gpt-5.6-luna",
        }
    }

    #[test]
    fn dispatch_run_event_empty_chain_returns_event_unchanged() {
        let hooks = HookSet::default();
        assert!(hooks.run_event_hooks_is_empty());
        let event = RunEvent::TextDelta { text: "hi".into() };
        let decision = hooks
            .dispatch_run_event(&event_ctx(), event.clone())
            .unwrap();
        assert_eq!(decision, Some(event));
    }

    #[test]
    fn dispatch_run_event_rewrite_publishes_rewritten_event() {
        struct UpperCase;
        impl RunEventHook for UpperCase {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                match event {
                    RunEvent::TextDelta { text } => Ok(Some(RunEvent::TextDelta {
                        text: text.to_uppercase(),
                    })),
                    other => Ok(Some(other)),
                }
            }
        }

        let hooks = HookSet::builder().run_event_hook(UpperCase).build();
        let decision = hooks
            .dispatch_run_event(&event_ctx(), RunEvent::TextDelta { text: "raw".into() })
            .unwrap();
        assert_eq!(decision, Some(RunEvent::TextDelta { text: "RAW".into() }));
    }

    #[test]
    fn dispatch_run_event_suppression_stops_the_chain() {
        struct SuppressText;
        impl RunEventHook for SuppressText {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                match event {
                    RunEvent::TextDelta { .. } => Ok(None),
                    other => Ok(Some(other)),
                }
            }
        }
        struct MustNotRun;
        impl RunEventHook for MustNotRun {
            fn hook(&self, _ctx: &RunEventContext<'_>, _event: RunEvent) -> RunEventHookResult {
                panic!("later hooks must not see a suppressed event");
            }
        }

        let hooks = HookSet::builder()
            .run_event_hook(SuppressText)
            .run_event_hook(MustNotRun)
            .build();
        let decision = hooks
            .dispatch_run_event(
                &event_ctx(),
                RunEvent::TextDelta {
                    text: "secret".into(),
                },
            )
            .unwrap();
        assert_eq!(decision, None);
    }

    #[test]
    fn dispatch_run_event_applies_hooks_in_registration_order() {
        struct Tag(&'static str);
        impl RunEventHook for Tag {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                match event {
                    RunEvent::TextDelta { text } => Ok(Some(RunEvent::TextDelta {
                        text: format!("{text}-{}", self.0),
                    })),
                    other => Ok(Some(other)),
                }
            }
        }

        let hooks = HookSet::builder()
            .run_event_hook(Tag("first"))
            .run_event_hook(Tag("second"))
            .build();
        let decision = hooks
            .dispatch_run_event(&event_ctx(), RunEvent::TextDelta { text: "x".into() })
            .unwrap();
        // The second hook must see the first hook's rewrite, proving
        // registration order.
        assert_eq!(
            decision,
            Some(RunEvent::TextDelta {
                text: "x-first-second".into()
            })
        );
    }

    #[test]
    fn dispatch_run_event_error_propagates_and_stops_the_chain() {
        struct Fail;
        impl RunEventHook for Fail {
            fn hook(&self, _ctx: &RunEventContext<'_>, _event: RunEvent) -> RunEventHookResult {
                Err(ToolError::validation("hook rejected the event"))
            }
        }
        struct MustNotRun;
        impl RunEventHook for MustNotRun {
            fn hook(&self, _ctx: &RunEventContext<'_>, _event: RunEvent) -> RunEventHookResult {
                panic!("later hooks must not run after a hook error");
            }
        }

        let hooks = HookSet::builder()
            .run_event_hook(Fail)
            .run_event_hook(MustNotRun)
            .build();
        let error = hooks
            .dispatch_run_event(&event_ctx(), RunEvent::OutputReady)
            .unwrap_err();
        assert!(matches!(error, ToolError::Validation { .. }));
    }

    #[test]
    fn hook_set_debug_includes_run_event_hooks_count() {
        struct Passthrough;
        impl RunEventHook for Passthrough {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                Ok(Some(event))
            }
        }

        let hooks = HookSet::builder().run_event_hook(Passthrough).build();
        let debug = format!("{hooks:?}");
        assert!(debug.contains("run_event_hooks: 1"));
    }

    // --- Compact dispatch tests -------------------------------------------------

    fn compact_history() -> Vec<CompactMessage> {
        vec![
            CompactMessage::new_preserved(RunMessageRole::System, "sys", Box::new("native-sys")),
            CompactMessage::new_preserved(RunMessageRole::User, "old", Box::new("native-old")),
            CompactMessage::new_preserved(
                RunMessageRole::Assistant,
                "recent",
                Box::new("native-recent"),
            ),
        ]
    }

    fn compact_result(summary: &str, messages_after: usize) -> CompactResult {
        CompactResult {
            summary: summary.into(),
            first_kept_entry_id: None,
            tokens_before: 100,
            tokens_after: 40,
            strategy: "summarize".into(),
            messages_before: 3,
            messages_after,
        }
    }

    /// Default-compaction stand-in: counts calls and records the
    /// entry texts it received. Rewrites the history to a summary
    /// message plus the newest entry.
    struct RecordingCompact {
        calls: AtomicUsize,
        seen: Mutex<Vec<String>>,
    }

    impl RecordingCompact {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen_texts(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl CompactExecutor for RecordingCompact {
        fn execute<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            mut messages: Vec<CompactMessage>,
        ) -> CompactHookFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                *self.seen.lock().unwrap() =
                    messages.iter().map(|m| m.text().to_string()).collect();
                let kept = messages.pop();
                messages.clear();
                messages.push(CompactMessage::new(RunMessageRole::User, "default summary"));
                if let Some(kept) = kept {
                    messages.push(kept);
                }
                Ok((
                    CompactOutcome::Compacted(compact_result("default", messages.len())),
                    messages,
                ))
            })
        }
    }

    fn compact_texts(history: &[CompactMessage]) -> Vec<String> {
        history.iter().map(|m| m.text().to_string()).collect()
    }

    #[tokio::test]
    async fn dispatch_compact_empty_chain_runs_default_executor_directly() {
        let hooks = HookSet::default();
        assert!(hooks.compact_hooks_is_empty());
        let executor = RecordingCompact::new();

        let (outcome, history) = hooks
            .dispatch_compact(&run_ctx(), compact_history(), &executor)
            .await
            .unwrap();

        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcome,
            CompactOutcome::Compacted(compact_result("default", 2))
        );
        assert_eq!(compact_texts(&history), vec!["default summary", "recent"]);
        // The untouched kept entry still carries its original
        // payload: pass-through loses nothing.
        assert!(history[1].has_preserved());
    }

    #[tokio::test]
    async fn dispatch_compact_runs_chain_in_registration_order_unwinding_in_reverse() {
        static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

        struct First;
        struct Second;

        impl CompactHook for First {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                LOG.lock().unwrap().push("first-before".into());
                Box::pin(async move {
                    let (outcome, history) = original.call(ctx, messages).await?;
                    // Post-`original` adjustment: an outer hook may
                    // rewrite the finished outcome before returning.
                    let outcome = match outcome {
                        CompactOutcome::Compacted(mut result) => {
                            result.summary.push_str("-unwound");
                            CompactOutcome::Compacted(result)
                        }
                        other => other,
                    };
                    LOG.lock().unwrap().push("first-after".into());
                    Ok((outcome, history))
                })
            }
        }

        impl CompactHook for Second {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                LOG.lock().unwrap().push("second-before".into());
                Box::pin(async move {
                    let (outcome, history) = original.call(ctx, messages).await?;
                    LOG.lock().unwrap().push("second-after".into());
                    Ok((outcome, history))
                })
            }
        }

        let executor = RecordingCompact::new();
        let hooks = HookSet::builder()
            .compact_hook(First)
            .compact_hook(Second)
            .build();
        LOG.lock().unwrap().clear();

        let (outcome, history) = hooks
            .dispatch_compact(&run_ctx(), compact_history(), &executor)
            .await
            .unwrap();

        // The outer hook's post-`original` adjustment is what the
        // dispatch caller receives.
        let mut expected = compact_result("default", 2);
        expected.summary.push_str("-unwound");
        assert_eq!(outcome, CompactOutcome::Compacted(expected));
        assert_eq!(
            *LOG.lock().unwrap(),
            vec![
                "first-before".to_string(),
                "second-before".to_string(),
                "second-after".to_string(),
                "first-after".to_string(),
            ]
        );
        // The default compaction's rewritten history is what the
        // caller receives through the unwound chain.
        assert_eq!(compact_texts(&history), vec!["default summary", "recent"]);
        assert!(history[1].has_preserved());
    }

    #[tokio::test]
    async fn dispatch_compact_before_original_mutations_reach_default_compaction() {
        struct Rewrite;

        impl CompactHook for Rewrite {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                mut messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                Box::pin(async move {
                    for message in &mut messages {
                        if message.text() == "old" {
                            message.set_text("rewritten");
                        }
                    }
                    original.call(ctx, messages).await
                })
            }
        }

        let executor = RecordingCompact::new();
        let hooks = HookSet::builder().compact_hook(Rewrite).build();

        hooks
            .dispatch_compact(&run_ctx(), compact_history(), &executor)
            .await
            .unwrap();

        // The default compaction consumed the hook's rewrite, not
        // the original text.
        assert_eq!(executor.seen_texts(), vec!["sys", "rewritten", "recent"]);
    }

    #[tokio::test]
    async fn dispatch_compact_skip_original_custom_result_skips_default_compaction() {
        struct CustomSummary;

        impl CompactHook for CustomSummary {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                mut messages: Vec<CompactMessage>,
                _original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                Box::pin(async move {
                    // The hook performs its own compaction: keep the
                    // newest entry, replace the rest with its summary.
                    let kept = messages.pop();
                    let mut compacted = Vec::with_capacity(2);
                    compacted.push(CompactMessage::new(RunMessageRole::User, "custom summary"));
                    if let Some(kept) = kept {
                        compacted.push(kept);
                    }
                    Ok((
                        CompactOutcome::Compacted(compact_result("custom", 2)),
                        compacted,
                    ))
                })
            }
        }

        let executor = RecordingCompact::new();
        let hooks = HookSet::builder().compact_hook(CustomSummary).build();

        let (outcome, history) = hooks
            .dispatch_compact(&run_ctx(), compact_history(), &executor)
            .await
            .unwrap();

        // The default compaction never ran for this attempt.
        assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            outcome,
            CompactOutcome::Compacted(compact_result("custom", 2))
        );
        assert_eq!(compact_texts(&history), vec!["custom summary", "recent"]);
    }

    #[tokio::test]
    async fn dispatch_compact_cancel_returns_history_unchanged() {
        struct Cancel;

        impl CompactHook for Cancel {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                _original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                Box::pin(async { Ok((CompactOutcome::Cancelled, messages)) })
            }
        }

        let executor = RecordingCompact::new();
        let hooks = HookSet::builder().compact_hook(Cancel).build();

        let (outcome, history) = hooks
            .dispatch_compact(&run_ctx(), compact_history(), &executor)
            .await
            .unwrap();

        assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
        assert_eq!(outcome, CompactOutcome::Cancelled);
        assert_eq!(compact_texts(&history), vec!["sys", "old", "recent"]);
        assert!(history.iter().all(|m| m.has_preserved()));
    }

    #[tokio::test]
    async fn dispatch_compact_hook_error_stops_the_chain() {
        struct FailingHook;
        impl CompactHook for FailingHook {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _messages: Vec<CompactMessage>,
                _original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                Box::pin(async { Err(ToolError::validation("hook rejected the compaction")) })
            }
        }

        struct MustNotRun;
        impl CompactHook for MustNotRun {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _messages: Vec<CompactMessage>,
                _original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                panic!("later hooks must not run after a hook error");
            }
        }

        let executor = RecordingCompact::new();
        let hooks = HookSet::builder()
            .compact_hook(FailingHook)
            .compact_hook(MustNotRun)
            .build();

        let result = hooks
            .dispatch_compact(&run_ctx(), compact_history(), &executor)
            .await;
        assert!(matches!(result, Err(ToolError::Validation { .. })));
        assert_eq!(executor.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn hook_set_debug_includes_compact_hooks_count() {
        struct Passthrough;
        impl CompactHook for Passthrough {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                original.call(ctx, messages)
            }
        }

        let hooks = HookSet::builder().compact_hook(Passthrough).build();
        let debug = format!("{hooks:?}");
        assert!(debug.contains("compact_hooks: 1"));
    }

    #[test]
    fn hook_set_debug_includes_run_hooks_count() {
        struct NoopRun;
        impl RunHook for NoopRun {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                _config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                original.call(ctx)
            }
        }
        let hooks = HookSet::builder().run_hook(NoopRun).build();
        let debug = format!("{:?}", hooks);
        assert!(debug.contains("run_hooks: 1"));
    }
}
