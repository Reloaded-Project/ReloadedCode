//! HookSetBuilder - builder for constructing a [`HookSet`].

use crate::hooks::{CompactHook, HookSet, RunConfigHook, RunEventHook, RunHook, ToolHook};
use std::fmt;
use std::sync::Arc;

/// Builder for constructing [`HookSet`].
#[derive(Default)]
pub struct HookSetBuilder {
    pub(super) tool_hooks: Vec<Arc<dyn ToolHook>>,
    pub(super) run_config_hooks: Vec<Arc<dyn RunConfigHook>>,
    pub(super) run_hooks: Vec<Arc<dyn RunHook>>,
    pub(super) run_event_hooks: Vec<Arc<dyn RunEventHook>>,
    pub(super) compact_hooks: Vec<Arc<dyn CompactHook>>,
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

    /// Registers a compact hook.
    ///
    /// Hooks run in registration order. Each hook's `original` handle calls
    /// the next registered hook, or the default compaction at the end of the
    /// chain.
    #[inline]
    #[must_use]
    pub fn compact_hook(mut self, hook: impl CompactHook) -> Self {
        self.compact_hooks.push(Arc::new(hook));
        self
    }

    /// Registers an already shared compact hook.
    #[inline]
    #[must_use]
    pub fn shared_compact_hook(mut self, hook: Arc<dyn CompactHook>) -> Self {
        self.compact_hooks.push(hook);
        self
    }

    /// Registers a run-config hook.
    ///
    /// Hooks run in registration order before the run hook chain,
    /// each mutating the run config in place.
    #[inline]
    #[must_use]
    pub fn run_config_hook(mut self, hook: impl RunConfigHook) -> Self {
        self.run_config_hooks.push(Arc::new(hook));
        self
    }

    /// Registers an already shared run-config hook.
    #[inline]
    #[must_use]
    pub fn shared_run_config_hook(mut self, hook: Arc<dyn RunConfigHook>) -> Self {
        self.run_config_hooks.push(hook);
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

    /// Registers a run-event hook.
    ///
    /// Hooks run in registration order on every streamed event,
    /// before the stream consumer sees it. Streaming path only:
    /// run-event hooks never fire during a non-streaming `run()`.
    #[inline]
    #[must_use]
    pub fn run_event_hook(mut self, hook: impl RunEventHook) -> Self {
        self.run_event_hooks.push(Arc::new(hook));
        self
    }

    /// Registers an already shared run-event hook.
    #[inline]
    #[must_use]
    pub fn shared_run_event_hook(mut self, hook: Arc<dyn RunEventHook>) -> Self {
        self.run_event_hooks.push(hook);
        self
    }

    /// Builds the `HookSet` from the configured hooks.
    #[inline]
    #[must_use]
    pub fn build(self) -> HookSet {
        HookSet {
            tool_hooks: self.tool_hooks,
            run_config_hooks: self.run_config_hooks,
            run_hooks: self.run_hooks,
            run_event_hooks: self.run_event_hooks,
            compact_hooks: self.compact_hooks,
        }
    }
}

impl fmt::Debug for HookSetBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookSetBuilder")
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
    use crate::hooks::compact_hook::{
        CompactHook, CompactHookFuture, CompactMessage, CompactOriginal,
    };
    use crate::hooks::run_event::{RunEvent, RunEventContext, RunEventHook, RunEventHookResult};
    use crate::hooks::run_hook::{
        HookRunContext, RunConfig, RunConfigHook, RunConfigHookFuture, RunHookFuture, RunOriginal,
    };
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
                _config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                original.call(ctx)
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
                _config: &'a RunConfig,
                original: RunOriginal<'a>,
            ) -> RunHookFuture<'a> {
                original.call(ctx)
            }
        }
        let shared: Arc<dyn RunHook> = Arc::new(NoopRun);
        let hooks = HookSetBuilder::new().shared_run_hook(shared).build();
        assert!(!hooks.run_hooks_is_empty());
        assert_eq!(hooks.run_hooks().len(), 1);
    }

    #[test]
    fn run_event_hook_registration_makes_hook_set_non_empty() {
        struct NoopEvent;
        impl RunEventHook for NoopEvent {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                Ok(Some(event))
            }
        }

        let hooks = HookSetBuilder::new().run_event_hook(NoopEvent).build();
        assert!(!hooks.is_empty());
        assert!(!hooks.run_event_hooks_is_empty());
    }

    #[test]
    fn shared_run_event_hook_registration() {
        struct NoopEvent;
        impl RunEventHook for NoopEvent {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                Ok(Some(event))
            }
        }

        let shared: Arc<dyn RunEventHook> = Arc::new(NoopEvent);
        let hooks = HookSetBuilder::new().shared_run_event_hook(shared).build();
        assert!(!hooks.run_event_hooks_is_empty());
    }

    #[test]
    fn run_config_hook_registration_makes_hook_set_non_empty() {
        struct NoopConfig;
        impl RunConfigHook for NoopConfig {
            fn configure<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a mut RunConfig,
            ) -> RunConfigHookFuture<'a> {
                Box::pin(async { Ok(()) })
            }
        }

        let hooks = HookSetBuilder::new().run_config_hook(NoopConfig).build();
        assert!(!hooks.is_empty());
        assert!(!hooks.run_config_hooks_is_empty());
        assert_eq!(hooks.run_config_hooks().len(), 1);
    }

    #[test]
    fn shared_run_config_hook_registration() {
        struct NoopConfig;
        impl RunConfigHook for NoopConfig {
            fn configure<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a mut RunConfig,
            ) -> RunConfigHookFuture<'a> {
                Box::pin(async { Ok(()) })
            }
        }

        let shared: Arc<dyn RunConfigHook> = Arc::new(NoopConfig);
        let hooks = HookSetBuilder::new().shared_run_config_hook(shared).build();
        assert!(!hooks.run_config_hooks_is_empty());
        assert_eq!(hooks.run_config_hooks().len(), 1);
    }

    #[test]
    // Pins manual Debug: counts only, never hook contents (traits lack Debug).
    fn builder_debug_includes_run_config_hooks() {
        struct NoopConfig;
        impl RunConfigHook for NoopConfig {
            fn configure<'a>(
                &'a self,
                _ctx: &'a HookRunContext<'a>,
                _config: &'a mut RunConfig,
            ) -> RunConfigHookFuture<'a> {
                Box::pin(async { Ok(()) })
            }
        }

        let builder = HookSetBuilder::new().run_config_hook(NoopConfig);
        let debug = format!("{builder:?}");
        assert!(debug.contains("run_config_hooks: 1"));
    }

    #[test]
    fn builder_debug_includes_run_hooks() {
        let builder = HookSetBuilder::new();
        let debug = format!("{:?}", builder);
        assert!(debug.contains("run_hooks"));
    }

    #[test]
    fn builder_debug_includes_run_event_hooks() {
        struct NoopEvent;
        impl RunEventHook for NoopEvent {
            fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
                Ok(Some(event))
            }
        }

        let builder = HookSetBuilder::new().run_event_hook(NoopEvent);
        let debug = format!("{builder:?}");
        assert!(debug.contains("run_event_hooks: 1"));
    }

    #[test]
    fn compact_hook_registration_makes_hook_set_non_empty() {
        struct NoopCompact;
        impl CompactHook for NoopCompact {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                original.call(ctx, messages)
            }
        }

        let hooks = HookSetBuilder::new().compact_hook(NoopCompact).build();
        assert!(!hooks.is_empty());
        assert!(!hooks.compact_hooks_is_empty());
    }

    #[test]
    fn shared_compact_hook_registration() {
        struct NoopCompact;
        impl CompactHook for NoopCompact {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                original.call(ctx, messages)
            }
        }

        let shared: Arc<dyn CompactHook> = Arc::new(NoopCompact);
        let hooks = HookSetBuilder::new().shared_compact_hook(shared).build();
        assert!(!hooks.compact_hooks_is_empty());
    }

    #[test]
    // Pins manual Debug: counts only, never hook contents (traits lack Debug).
    fn builder_debug_includes_compact_hooks() {
        struct NoopCompact;
        impl CompactHook for NoopCompact {
            fn hook<'a>(
                &'a self,
                ctx: &'a HookRunContext<'a>,
                messages: Vec<CompactMessage>,
                original: CompactOriginal<'a>,
            ) -> CompactHookFuture<'a> {
                original.call(ctx, messages)
            }
        }

        let builder = HookSetBuilder::new().compact_hook(NoopCompact);
        let debug = format!("{builder:?}");
        assert!(debug.contains("compact_hooks: 1"));
    }
}
