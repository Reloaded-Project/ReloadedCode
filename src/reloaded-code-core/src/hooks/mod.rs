//! Hook infrastructure for tool hooks, run lifecycle hooks, and run
//! event hooks.
//!
//! # Public API
//!
//! Tool hook types:
//! - [`ToolHook`] - Intercepts a tool call and may call [`ToolOriginal`]
//! - [`ToolHookFuture`] - Boxed future returned by [`ToolHook::hook`]
//! - [`ToolOriginal`] - Managed trampoline to the next hook or real tool
//! - [`ToolCallContext`] - Tool name, agent name, and run id
//! - [`ToolRequest`] - JSON tool arguments
//! - [`ToolExecutor`] - Final callable used at the end of the hook chain
//!
//! Run hook types:
//! - [`RunConfigHook`] - Amends a run's config before the run starts
//! - [`RunConfigHookFuture`] - Boxed future returned by [`RunConfigHook::configure`]
//! - [`RunHook`] - Intercepts a run and may call [`RunOriginal`]
//! - [`RunHookFuture`] - Boxed future returned by [`RunHook::hook`]
//! - [`RunOriginal`] - Managed trampoline to the next hook or real run executor
//! - [`RunConfig`] - Config a run config hook amends before a run; the run chain observes it read-only
//! - [`RunOutput`] - Result of a completed run
//! - [`RunExecutor`] - Final callable used at the end of the run hook chain
//! - [`HookRunContext`] - Context given to hook run lifecycle events
//! - [`EndReason`] - Why a run ended
//!
//! Run event types:
//! - [`RunEvent`] - Framework-owned event yielded by run streams
//! - [`RunMessage`] - Distilled transcript message in a completed run
//! - [`RunMessageRole`] - Author role of a transcript message
//! - [`RunToolCallSummary`] - Distilled tool call summary
//! - [`RunToolResultSummary`] - Distilled tool result summary
//!
//! Run event hook types:
//! - [`RunEventHook`] - Observes, rewrites, or suppresses streamed run events
//! - [`RunEventContext`] - Agent and model names for a run-event hook call
//!
//! Observers are plain hooks: code before `original` is "start", code
//! after is "end". They participate in the same hook chain.
//!
//! Container:
//! - [`HookSet`] - Container for registered hooks and lifecycle events
//! - [`HookSetBuilder`] - Builder for constructing [`HookSet`]
//!
//! # Design
//!
//! Tool hooks and run hooks follow game-style hook semantics. Each hook
//! receives an `original` handle. Calling it invokes the next hook in the
//! chain, or the real implementation when the chain is exhausted. Not calling
//! it blocks or replaces the call. Everything is built on top of the same
//! hook chain.

pub use self::builder::HookSetBuilder;
pub use self::hook_set::HookSet;
pub use self::run_event::*;
pub use self::run_hook::*;
pub use self::tool_hook::*;

mod builder;
mod hook_set;
mod run_event;
mod run_hook;
mod tool_hook;

/// Max hooks per point before falling back to heap.
pub(crate) const INLINE_CAP: usize = 3;
