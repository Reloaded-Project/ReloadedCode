//! Hook infrastructure for tool hooks and run lifecycle hooks.
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
//! - [`RunHook`] - Intercepts a run and may call [`RunOriginal`]
//! - [`RunHookFuture`] - Boxed future returned by [`RunHook::hook`]
//! - [`RunOriginal`] - Managed trampoline to the next hook or real run executor
//! - [`RunConfig`] - Mutable config a RunHook can change before calling original
//! - [`RunOutput`] - Result of a completed run
//! - [`RunExecutor`] - Final callable used at the end of the run hook chain
//!
//! Notification callbacks e.g. (`on_run_start` / `on_run_end`) are
//! implemented as lightweight `Hook` wrappers. They participate in the
//! same hook chain: code before `original` is "start", code after is "end".
//!
//! Hook context types:
//! - [`HookRunContext`] - Context given to hook run lifecycle events
//! - [`EndReason`] - Why a run ended
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

mod builder;
mod hook_set;
mod run_hook;
mod session;
mod tool_hook;

pub use self::builder::HookSetBuilder;
pub use self::hook_set::HookSet;
pub use self::run_hook::*;
pub use self::session::*;
pub use self::tool_hook::*;

/// Max hooks per point before falling back to heap.
pub(crate) const INLINE_CAP: usize = 3;
