//! HookSet — container and dispatch for all registered hooks and lifecycle events.

use crate::hooks::{
    HookRunContext, RunConfig, RunExecutor, RunHook, RunHookFuture, RunOriginal, SessionCompactFn,
    ToolCallContext, ToolExecutor, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest, INLINE_CAP,
};
use std::fmt;
use std::sync::Arc;
use tinyvec::TinyVec;

/// All registered hooks and lifecycle events, stored per point.
#[derive(Clone, Default)]
pub struct HookSet {
    pub(super) tool_hooks: Vec<Arc<dyn ToolHook>>,
    pub(super) run_hooks: Vec<Arc<dyn RunHook>>,
    pub(super) session_compact: TinyVec<[Option<SessionCompactFn>; INLINE_CAP]>,
}

impl HookSet {
    /// Returns `true` if no hooks are registered at any point.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tool_hooks.is_empty() && self.run_hooks.is_empty() && self.session_compact.is_empty()
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

    /// Returns registered tool hooks in dispatch order.
    #[inline]
    #[must_use]
    pub fn tool_hooks(&self) -> &[Arc<dyn ToolHook>] {
        &self.tool_hooks
    }

    /// Returns registered run hooks in dispatch order.
    ///
    /// Includes wrappers created by `on_run_start` / `on_run_end`.
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
    /// Includes `on_run_start` / `on_run_end` wrappers registered via the
    /// builder. If no run hooks are registered, this calls the real run
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
            .field("session_compact", &self.session_compact.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::run_hook::{
        RunConfig, RunExecutor, RunHook, RunHookFuture, RunOriginal, RunOutput, RunUsage,
    };
    use crate::hooks::session::EndReason;
    use crate::ToolOutput;
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

    // --- Run notify wrappers via dispatch_run ---------------------------------

    #[test]
    fn on_run_start_wrapper_counts_as_run_hook() {
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .on_run_start(|_ctx| {})
            .build();
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[test]
    fn on_run_end_wrapper_counts_as_run_hook() {
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .on_run_end(|_ctx, _reason| {})
            .build();
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[tokio::test]
    async fn on_run_start_fires_before_real_executor() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "done".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        COUNTER.store(0, Ordering::SeqCst);
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .on_run_start(|_ctx| {
                COUNTER.fetch_add(1, Ordering::SeqCst);
            })
            .build();
        let ctx = HookRunContext {
            agent_name: "a",
            run_id: "r1",
            model_name: "m",
        };
        let output = hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(output.content, "done");
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn on_run_end_receives_end_reason() {
        static REASON: std::sync::Mutex<Option<EndReason>> = std::sync::Mutex::new(None);

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

        *REASON.lock().unwrap() = None;
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .on_run_end(|_ctx, reason| {
                *REASON.lock().unwrap() = Some(reason);
            })
            .build();
        let ctx = HookRunContext {
            agent_name: "a",
            run_id: "r1",
            model_name: "m",
        };
        hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(*REASON.lock().unwrap(), Some(EndReason::Completed));
    }

    #[tokio::test]
    async fn on_run_end_receives_failed_reason() {
        static REASON: std::sync::Mutex<Option<EndReason>> = std::sync::Mutex::new(None);

        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "fail".into(),
                        reason: EndReason::Failed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        *REASON.lock().unwrap() = None;
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .on_run_end(|_ctx, reason| {
                *REASON.lock().unwrap() = Some(reason);
            })
            .build();
        let ctx = HookRunContext {
            agent_name: "a",
            run_id: "r1",
            model_name: "m",
        };
        hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(*REASON.lock().unwrap(), Some(EndReason::Failed));
    }

    #[tokio::test]
    async fn on_run_end_does_not_fire_when_hook_before_it_skips() {
        static END_FIRED: AtomicUsize = AtomicUsize::new(0);

        struct SkipEverything;
        impl RunHook for SkipEverything {
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

        END_FIRED.store(0, Ordering::SeqCst);
        let hooks = crate::hooks::builder::HookSetBuilder::new()
            .run_hook(SkipEverything)
            .on_run_end(|_ctx, _reason| {
                END_FIRED.fetch_add(1, Ordering::SeqCst);
            })
            .build();
        let ctx = HookRunContext {
            agent_name: "a",
            run_id: "r1",
            model_name: "m",
        };

        // RealRun should never execute
        struct RealRun;
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

        hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(END_FIRED.load(Ordering::SeqCst), 0);
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

    #[tokio::test]
    async fn on_run_start_fires_before_other_hooks() {
        use std::sync::Mutex;
        static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

        struct Echo;
        impl RunHook for Echo {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                LOG.lock().unwrap().push("hook-before".into());
                Box::pin(async move {
                    let output = original.call(ctx, config).await?;
                    LOG.lock().unwrap().push("hook-after".into());
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
        let hooks = HookSet::builder()
            .on_run_start(|_ctx| {
                LOG.lock().unwrap().push("start-callback".into());
            })
            .run_hook(Echo)
            .build();

        let ctx = HookRunContext {
            agent_name: "a",
            run_id: "r1",
            model_name: "m",
        };
        hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        let log = LOG.lock().unwrap();
        assert_eq!(*log, vec!["start-callback", "hook-before", "hook-after"]);
    }

    #[tokio::test]
    async fn on_run_end_fires_after_chain_completes() {
        use std::sync::Mutex;
        static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

        struct RealRun;
        impl RunExecutor for RealRun {
            fn execute<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: RunConfig,
            ) -> RunHookFuture<'a> {
                Box::pin(async {
                    Ok(RunOutput {
                        content: "done".into(),
                        reason: EndReason::Completed,
                        usage: RunUsage::default(),
                    })
                })
            }
        }

        LOG.lock().unwrap().clear();
        let hooks = HookSet::builder()
            .on_run_end(|_ctx, _reason| {
                LOG.lock().unwrap().push("end-callback".into());
            })
            .build();

        let ctx = HookRunContext {
            agent_name: "a",
            run_id: "r1",
            model_name: "m",
        };
        let output = hooks
            .dispatch_run(&ctx, RunConfig::default(), &RealRun)
            .await
            .unwrap();

        assert_eq!(output.content, "done");
        assert_eq!(*LOG.lock().unwrap(), vec!["end-callback"]);
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
