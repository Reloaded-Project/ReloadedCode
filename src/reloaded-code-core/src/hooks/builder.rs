//! HookSetBuilder — builder for constructing a [`HookSet`].

use crate::hooks::{
    EndReason, HookRunContext, HookSet, RunConfig, RunHook, RunHookFuture, RunOriginal,
    SessionCompactFn, ToolHook, INLINE_CAP,
};
use std::fmt;
use std::sync::Arc;
use tinyvec::TinyVec;

/// Builder for constructing [`HookSet`].
#[derive(Default)]
pub struct HookSetBuilder {
    pub(super) tool_hooks: Vec<Arc<dyn ToolHook>>,
    pub(super) run_hooks: Vec<Arc<dyn RunHook>>,
    pub(super) session_compact: TinyVec<[Option<SessionCompactFn>; INLINE_CAP]>,
}

impl HookSetBuilder {
    /// Creates a new, empty builder.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a game-style tool hook.
    ///
    /// Hooks run in registration order. Each hook's `original` handle calls
    /// the next registered hook, or the real tool at the end of the chain.
    #[inline]
    #[must_use]
    pub fn tool_hook(mut self, hook: impl ToolHook) -> Self {
        self.tool_hooks.push(Arc::new(hook));
        self
    }

    /// Registers an already shared game-style tool hook.
    #[inline]
    #[must_use]
    pub fn shared_tool_hook(mut self, hook: Arc<dyn ToolHook>) -> Self {
        self.tool_hooks.push(hook);
        self
    }

    /// Registers a run-start observer as a `RunHook` wrapper.
    #[inline]
    #[must_use]
    pub fn on_run_start(mut self, callback: for<'a> fn(&'a HookRunContext<'a>)) -> Self {
        struct RunStartWrapper {
            callback: for<'a> fn(&'a HookRunContext<'a>),
        }

        impl RunHook for RunStartWrapper {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async move {
                    (self.callback)(ctx);
                    original.call(ctx, config).await
                })
            }
        }

        self.run_hooks.push(Arc::new(RunStartWrapper { callback }));
        self
    }

    /// Registers a run-end observer as a `RunHook` wrapper.
    #[inline]
    #[must_use]
    pub fn on_run_end(mut self, callback: for<'a> fn(&'a HookRunContext<'a>, EndReason)) -> Self {
        struct RunEndWrapper {
            callback: for<'a> fn(&'a HookRunContext<'a>, EndReason),
        }

        impl RunHook for RunEndWrapper {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                config: RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                Box::pin(async move {
                    let output = original.call(ctx, config).await?;
                    (self.callback)(ctx, output.reason);
                    Ok(output)
                })
            }
        }

        self.run_hooks.push(Arc::new(RunEndWrapper { callback }));
        self
    }

    /// Registers a compact event. Name preserved — compact is its own concept, distinct from "run".
    #[inline]
    #[must_use]
    pub fn on_session_compact(mut self, event: SessionCompactFn) -> Self {
        self.session_compact.push(Some(event));
        self
    }

    /// Registers a game-style run hook.
    ///
    /// Hooks run in registration order. Each hook's `original` handle calls
    /// the next registered hook, or the real run executor at the end of the chain.
    #[inline]
    #[must_use]
    pub fn run_hook(mut self, hook: impl RunHook) -> Self {
        self.run_hooks.push(Arc::new(hook));
        self
    }

    /// Registers an already shared game-style run hook.
    #[inline]
    #[must_use]
    pub fn shared_run_hook(mut self, hook: Arc<dyn RunHook>) -> Self {
        self.run_hooks.push(hook);
        self
    }

    /// Builds the `HookSet` from the configured hooks.
    #[inline]
    #[must_use]
    pub fn build(self) -> HookSet {
        HookSet {
            tool_hooks: self.tool_hooks,
            run_hooks: self.run_hooks,
            session_compact: self.session_compact,
        }
    }
}

impl fmt::Debug for HookSetBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookSetBuilder")
            .field("tool_hooks", &self.tool_hooks.len())
            .field("run_hooks", &self.run_hooks.len())
            .field("session_compact", &self.session_compact.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::run_hook::{RunConfig, RunHookFuture, RunOriginal};
    use crate::hooks::session::HookRunContext;
    use crate::hooks::tool_hook::{ToolCallContext, ToolHookFuture, ToolOriginal, ToolRequest};

    #[test]
    fn hook_set_builder_new_produces_empty() {
        let hooks = HookSetBuilder::new().build();
        assert!(hooks.is_empty());
    }

    #[test]
    fn hook_set_builder_roundtrip() {
        let hooks = HookSet::builder().build();
        assert!(hooks.is_empty());
    }

    #[test]
    fn tool_hook_registration_makes_hook_set_non_empty() {
        struct Noop;

        impl ToolHook for Noop {
            fn hook<'a>(
                &'a self,
                ctx: &'a ToolCallContext<'a>,
                req: ToolRequest,
                original: ToolOriginal<'a>,
            ) -> ToolHookFuture<'a> {
                original.call(ctx, req)
            }
        }

        let hooks = HookSetBuilder::new().tool_hook(Noop).build();
        assert!(!hooks.is_empty());
        assert!(!hooks.tool_hooks_is_empty());
        assert_eq!(hooks.tool_hooks().len(), 1);
    }

    #[test]
    fn run_hook_registration_makes_hook_set_non_empty() {
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
        let hooks = HookSetBuilder::new().run_hook(NoopRun).build();
        assert!(!hooks.is_empty());
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[test]
    fn shared_run_hook_registration() {
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
        let shared: Arc<dyn RunHook> = Arc::new(NoopRun);
        let hooks = HookSetBuilder::new().shared_run_hook(shared).build();
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[test]
    fn on_run_start_registers_a_run_hook_wrapper() {
        let hooks = HookSetBuilder::new().on_run_start(|_ctx| {}).build();
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[test]
    fn on_run_end_registers_a_run_hook_wrapper() {
        let hooks = HookSetBuilder::new().on_run_end(|_ctx, _reason| {}).build();
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[test]
    fn builder_debug_includes_run_hooks() {
        let builder = HookSetBuilder::new();
        let debug = format!("{:?}", builder);
        assert!(debug.contains("run_hooks"));
    }
}
