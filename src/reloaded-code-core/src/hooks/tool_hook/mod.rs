//! Tool hook types: intercept trait, context, request, and chain trampoline.
//!
//! # What a tool call is
//!
//! One tool call = one invocation of a single tool. The model requests
//! the tool with JSON arguments, the tool runs, a result goes back to
//! the model. Each request during a run is its own call.
//!
//! Tool hooks fire inside a run, once per tool call, under the same
//! `run_id` as the enclosing run.
//!
//! [`ToolCallContext`] names the call: `tool_name` for the tool being
//! called, `agent_name` for the agent making the call, `run_id` for
//! the enclosing run.
//!
//! A tool hook wraps that single call. Code before
//! [`ToolOriginal::call`] sees the raw [`ToolRequest`]: inspect the
//! JSON arguments or rewrite them.
//!
//! Code after [`ToolOriginal::call`] sees the result returned by the
//! next hook in the chain, or the real tool if none remain: an inner
//! hook may wrap or replace it. Skipping `original` blocks the call:
//! the real tool never runs and the hook's return value becomes the
//! result.
//!
//! [`ToolOriginal`] is consumed by [`ToolOriginal::call`], so each hook
//! can continue the chain at most once. There is no built-in retry: a
//! hook that wants to block or replace the call returns its own result
//! without calling the continuation.
//!
//! Multiple hooks run in registration order, outer-to-inner: the first
//! registered hook is outermost, the last one sits directly on the
//! real tool.
//!
//! Next: see [`RunHook`] for the whole-run intercept point. Config
//! changes belong to [`RunConfigHook`], the one run-level hook
//! point that fires on both run paths.
//!
//! [`RunConfigHook`]: crate::hooks::RunConfigHook
//! [`RunHook`]: crate::hooks::RunHook

use crate::{ToolOutput, ToolResult};
use serde_json::Value;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Context passed to each tool hook.
#[derive(Debug)]
pub struct ToolCallContext<'a> {
    /// Name of the tool being called.
    pub tool_name: &'static str,
    /// Name of the agent making the call.
    pub agent_name: &'a str,
    /// Unique identifier for the current run.
    pub run_id: &'a str,
}

/// Boxed future returned by [`ToolHook::hook`] and [`ToolExecutor::execute`].
pub type ToolHookFuture<'a> = Pin<Box<dyn Future<Output = ToolResult<ToolOutput>> + Send + 'a>>;

/// Managed trampoline to the next hook or real tool.
///
/// `ToolOriginal` is consumed by [`call`], so each hook can continue
/// the chain at most once. There is no built-in retry: block or replace
/// the call by returning a result without invoking the continuation.
///
/// [`call`]: Self::call
pub struct ToolOriginal<'a> {
    chain: &'a [Arc<dyn ToolHook>],
    index: usize,
    real_tool: &'a dyn ToolExecutor,
}

/// Request passed through the tool hook chain.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRequest {
    /// JSON arguments passed to the tool.
    pub args: Value,
}

/// Final callable used when the hook chain reaches the real tool.
pub trait ToolExecutor: Send + Sync {
    /// Executes the real tool.
    fn execute<'a>(&'a self, ctx: &'a ToolCallContext<'a>, req: ToolRequest) -> ToolHookFuture<'a>;
}

/// Game-style tool hook.
///
/// A hook may inspect or change the request, call [`ToolOriginal::call`] to
/// continue, inspect or change the response, or skip `original` entirely to
/// block/replace the tool call.
pub trait ToolHook: Send + Sync + 'static {
    /// Intercepts a tool call.
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a>;
}

impl<'a> ToolOriginal<'a> {
    /// Creates a trampoline over the provided hook chain and real tool.
    #[inline]
    #[must_use]
    pub fn new(chain: &'a [Arc<dyn ToolHook>], real_tool: &'a dyn ToolExecutor) -> Self {
        Self {
            chain,
            index: 0,
            real_tool,
        }
    }

    /// Calls the next hook, or the real tool when no hooks remain.
    #[inline]
    pub fn call(self, ctx: &'a ToolCallContext<'a>, req: ToolRequest) -> ToolHookFuture<'a> {
        if let Some(hook) = self.chain.get(self.index) {
            hook.hook(
                ctx,
                req,
                Self {
                    chain: self.chain,
                    index: self.index + 1,
                    real_tool: self.real_tool,
                },
            )
        } else {
            self.real_tool.execute(ctx, req)
        }
    }
}

impl ToolRequest {
    /// Creates a request from JSON arguments.
    #[inline]
    #[must_use]
    pub fn new(args: Value) -> Self {
        Self { args }
    }
}

impl fmt::Debug for ToolOriginal<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolOriginal")
            .field("chain_len", &self.chain.len())
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl From<Value> for ToolRequest {
    #[inline]
    fn from(args: Value) -> Self {
        Self::new(args)
    }
}

impl<F> ToolExecutor for F
where
    F: for<'a> Fn(&'a ToolCallContext<'a>, ToolRequest) -> ToolHookFuture<'a> + Send + Sync,
{
    #[inline]
    fn execute<'a>(&'a self, ctx: &'a ToolCallContext<'a>, req: ToolRequest) -> ToolHookFuture<'a> {
        self(ctx, req)
    }
}

impl<F> ToolHook for F
where
    F: for<'a> Fn(&'a ToolCallContext<'a>, ToolRequest, ToolOriginal<'a>) -> ToolHookFuture<'a>
        + Send
        + Sync
        + 'static,
{
    #[inline]
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        self(ctx, req, original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_request_carries_args() {
        let req = ToolRequest::new(json!({"path": "/tmp/x"}));
        assert_eq!(req.args, json!({"path": "/tmp/x"}));
    }

    #[test]
    fn tool_call_context_fields_are_accessible() {
        let ctx = ToolCallContext {
            tool_name: "read_file",
            agent_name: "planner",
            run_id: "r1",
        };
        assert_eq!(ctx.tool_name, "read_file");
        assert_eq!(ctx.agent_name, "planner");
        assert_eq!(ctx.run_id, "r1");
    }
}
