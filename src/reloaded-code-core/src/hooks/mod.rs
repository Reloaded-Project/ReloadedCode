//! Hook infrastructure for tool hooks and session lifecycle events.
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
//! Session event types:
//! - [`SessionContext`] - Context given to session lifecycle events
//! - [`EndReason`] - Why a session ended
//!
//! Container:
//! - [`HookSet`] - Container for registered hooks and lifecycle events
//! - [`HookSetBuilder`] - Builder for constructing [`HookSet`]
//!
//! # Design
//!
//! Tool hooks follow game-style hook semantics. Each hook receives an
//! `original` handle. Calling it invokes the next hook in the chain, or the
//! real tool when the chain is exhausted. Not calling it blocks or replaces the
//! tool call. Session hooks remain simple lifecycle events.

pub use self::builder::HookSetBuilder;
pub use self::hook_set::HookSet;
pub use self::session::*;
pub use self::tool_hook::*;

mod builder;
mod hook_set;
mod session;
mod tool_hook;

/// Max hooks per point before falling back to heap.
pub(crate) const INLINE_CAP: usize = 3;
