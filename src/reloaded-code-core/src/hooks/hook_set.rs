//! HookSet — container and dispatch for all registered hooks and lifecycle events.

use crate::hooks::{
    HookRunContext, RunConfig, RunEvent, RunEventContext, RunEventHook, RunEventHookResult,
    RunExecutor, RunHook, RunHookFuture, RunOriginal, SessionCompactFn, ToolCallContext,
    ToolExecutor, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest, INLINE_CAP,
};
use std::fmt;
use std::sync::Arc;
use tinyvec::TinyVec;

/// All registered hooks and lifecycle events, stored per point.
#[derive(Clone, Default)]
pub struct HookSet {
    pub(super) tool_hooks: Vec<Arc<dyn ToolHook>>,
    pub(super) run_hooks: Vec<Arc<dyn RunHook>>,
    pub(super) run_event_hooks: Vec<Arc<dyn RunEventHook>>,
    pub(super) session_compact: TinyVec<[Option<SessionCompactFn>; INLINE_CAP]>,
}

impl HookSet {
    /// Returns `true` if no hooks are registered at any point.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tool_hooks.is_empty()
            && self.run_hooks.is_empty()
            && self.run_event_hooks.is_empty()
            && self.session_compact.is_empty()
    }

    /// Returns `true` if no tool hooks are registered.
    #[inline]
    #[must_use]
    pub fn tool_hooks_is_empty(&self) -> bool {
        self.tool_hooks.is_empty()
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

    /// Returns registered tool hooks in dispatch order.
    #[inline]
    #[must_use]
    pub fn tool_hooks(&self) -> &[Arc<dyn ToolHook>] {
        &self.tool_hooks
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

    /// Dispatches a run through the hook chain.
    ///
    /// If no run hooks are registered, this calls the real run
    /// executor directly.
    ///
    /// # Errors
    /// Returns `ToolError` if the executor or any run hook in the chain returns an error.
    #[inline]
    pub fn dispatch_run<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: RunConfig,
        real_run: &'a dyn RunExecutor,
    ) -> RunHookFuture<'a> {
        if self.run_hooks.is_empty() {
            return real_run.execute(ctx, config);
        }
        RunOriginal::new(&self.run_hooks, real_run).call(ctx, config)
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

    /// Dispatches compact events. Name preserved — compact is its own concept, distinct from "run".
    #[inline]
    pub fn dispatch_session_compact(&self, ctx: &HookRunContext<'_>) {
        for event in self.session_compact.iter().flatten() {
            event(ctx);
        }
    }
}

impl fmt::Debug for HookSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookSet")
            .field("tool_hooks", &self.tool_hooks.len())
            .field("run_hooks", &self.run_hooks.len())
            .field("run_event_hooks", &self.run_event_hooks.len())
            .field("session_compact", &self.session_compact.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::run_event::{RunEvent, RunEventContext, RunEventHook, RunEventHookResult};
    use crate::hooks::run_hook::{
        EndReason, RunConfig, RunExecutor, RunHook, RunHookFuture, RunOriginal, RunOutput, RunUsage,
    };
    use crate::{ToolError, ToolOutput};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn ready(output: impl Into<ToolOutput>) -> ToolHookFuture<'static> {
        let output = output.into();
        Box::pin(async move { Ok(output) })
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
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                original.call(ctx, config)
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
        struct Prefix;
        struct RealRun;

        impl RunHook for Prefix {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                mut config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async move {
                    config.system_prompt = Some("overridden".into());
                    let mut output = original.call(ctx, config).await?;
                    output.content.push_str("-post");
                    Ok(output)
                })
            }
        }

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
            .run_hook(Prefix)
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

        assert_eq!(output.content, "overridden-post");
        assert_eq!(output.reason, EndReason::Completed);
    }

    #[tokio::test]
    async fn dispatch_run_hook_can_skip_without_calling_original() {
        struct Skip;
        struct RealRun;

        impl RunHook for Skip {
            fn hook<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
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
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                LOG.lock().unwrap().push("first-before".into());
                Box::pin(async move {
                    let output = original.call(ctx, config).await?;
                    LOG.lock().unwrap().push("first-after".into());
                    Ok(output)
                })
            }
        }

        impl RunHook for Second {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                LOG.lock().unwrap().push("second-before".into());
                Box::pin(async move {
                    let output = original.call(ctx, config).await?;
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

    #[tokio::test]
    async fn session_compact_dispatch_untouched() {
        static COMPACTS: AtomicUsize = AtomicUsize::new(0);

        fn on_compact(_ctx: &HookRunContext<'_>) {
            COMPACTS.fetch_add(1, Ordering::SeqCst);
        }

        COMPACTS.store(0, Ordering::SeqCst);
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .on_session_compact(on_compact)
            .build();
        let ctx = HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        };

        hooks.dispatch_session_compact(&ctx);
        assert_eq!(COMPACTS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn hook_set_debug_includes_run_hooks_count() {
        struct NoopRun;
        impl RunHook for NoopRun {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                original.call(ctx, config)
            }
        }
        let hooks = HookSet::builder().run_hook(NoopRun).build();
        let debug = format!("{:?}", hooks);
        assert!(debug.contains("run_hooks: 1"));
    }
}
