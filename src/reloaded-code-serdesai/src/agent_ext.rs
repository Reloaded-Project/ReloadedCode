//! Extension traits for integrating tools with serdes-ai AgentBuilder.
//!
//! This module provides adapters to use [`Tool`] implementations with
//! serdes-ai's [`AgentBuilder`].
//!
//! # Example
//!
//! ```no_run
//! use reloaded_code_serdesai::{ReadTool, GlobTool, AbsolutePathResolver};
//! use reloaded_code_serdesai::agent_ext::AgentBuilderExt;
//! use serdes_ai::prelude::*;
//!
//! # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! let agent = AgentBuilder::<(), String>::from_model("openai:gpt-5.4")?
//!     .tool(ReadTool::new(AbsolutePathResolver))
//!     .tool(GlobTool::new(AbsolutePathResolver))
//!     .system_prompt("You are helpful.")
//!     .build();
//! # Ok(())
//! # }
//! ```

use crate::AgentBuildError;
use async_trait::async_trait;
use reloaded_code_core::hooks::{
    HookSet, ToolCallContext, ToolExecutor as CoreToolExecutor, ToolHookFuture, ToolRequest,
};
use serde_json::Value as JsonValue;
use serdes_ai::agent::ToolExecutor;
use serdes_ai::tools::{RunContext as ToolsRunContext, Tool, ToolError, ToolReturn};
use serdes_ai::{AgentBuilder, RunContext as AgentRunContext};
use std::sync::Arc;

/// Original tool result captured by [`CoreToolBridge`] while the hook chain
/// runs. Lets [`HookedToolExecutor`] restore the untouched `ToolReturn` or
/// `ToolError` after dispatch, so JSON shapes, truncated markers, image
/// content, `tool_call_id`, and structured validation errors reach the model
/// exactly as the no-hook path would deliver them.
#[derive(Default)]
struct CapturedToolResult {
    return_value: std::sync::Mutex<Option<ToolReturn>>,
    error: std::sync::Mutex<Option<ToolError>>,
}

/// Bridges a SerdesAI `ToolExecutor` back to the core `ToolExecutor` trait so
/// [`HookSet::dispatch_tool`] can call the real tool at the end of the hook chain.
///
/// Borrows the original execution context to avoid cloning strings and settings.
struct CoreToolBridge<'a, Deps> {
    inner: &'a dyn serdes_ai::agent::ToolExecutor<Deps>,
    ctx: &'a AgentRunContext<Deps>,
    captured: &'a CapturedToolResult,
}

/// Adapter for boxed trait object tools, similar to [`ToolAsExecutor`] but
/// for dynamically dispatched tools where the concrete type is not known
/// at compile time.
pub(crate) struct DynToolAsExecutor<Deps>(pub(crate) Box<dyn Tool<Deps> + Send + Sync>);

/// Wraps a SerdesAI [`ToolExecutor`] so that [`HookSet::dispatch_tool`] is called
/// before the inner executor when tool hooks are registered. Pass-through when
/// no tool hooks are present.
pub(crate) struct HookedToolExecutor<Deps> {
    inner: Arc<dyn serdes_ai::agent::ToolExecutor<Deps> + Send + Sync>,
    hooks: HookSet,
    agent_name: String,
    tool_name: &'static str,
}

/// Adapter that wraps a [`Tool`] to implement [`ToolExecutor`].
///
/// This bridges the gap between `serdes_ai::tools::Tool` (which uses
/// `tools::RunContext`) and `serdes_ai::agent::ToolExecutor` (which uses
/// `agent::RunContext`).
pub(crate) struct ToolAsExecutor<T>(T);

/// Extension trait for [`AgentBuilder`] to add tools that implement [`Tool`].
pub trait AgentBuilderExt<Deps, Output> {
    /// Add a tool that implements the [`Tool`] trait.
    ///
    /// This is a convenience method that extracts the tool's definition
    /// and wraps it with an executor adapter.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use reloaded_code_serdesai::{ReadTool, GlobTool, AbsolutePathResolver};
    /// use reloaded_code_serdesai::agent_ext::AgentBuilderExt;
    /// use serdes_ai::prelude::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let agent = AgentBuilder::<(), String>::from_model("openai:gpt-5.4")?
    ///     .tool(ReadTool::new(AbsolutePathResolver))
    ///     .tool(GlobTool::new(AbsolutePathResolver))
    ///     .build();
    /// # Ok(())
    /// # }
    /// ```
    fn tool<T: Tool<Deps> + 'static>(self, tool: T) -> Self;

    /// Add a boxed trait object tool.
    ///
    /// This is useful for dynamically created tools where the concrete type
    /// is not known at compile time (e.g., custom tools from a factory).
    fn tool_dyn(
        self,
        definition: serdes_ai::ToolDefinition,
        tool: Box<dyn Tool<Deps> + Send + Sync>,
    ) -> Self;
}

/// Extension for converting [`ToolError`] results into [`AgentBuildError`].
///
/// This avoids repeating the full `ToolSettingsValidation` struct literal at
/// every `.map_err` call site.
///
/// # Example
///
/// ```no_run
/// use reloaded_code_serdesai::agent_ext::ToolResultExt;
/// # use reloaded_code_serdesai::AgentBuildError;
/// # fn demo(r: Result<usize, reloaded_code_core::ToolError>) -> Result<(), AgentBuildError> {
/// let value = r.with_tool("my_tool")?;
/// # Ok(())
/// # }
/// ```
pub trait ToolResultExt<T> {
    /// Maps a [`ToolError`] to
    /// [`AgentBuildError::ToolSettingsValidation`].
    ///
    /// # Errors
    /// - Returns [`AgentBuildError::ToolSettingsValidation`] when the original result
    ///   contains a [`ToolError`], preserving the tool name and original error.
    ///
    /// [`ToolError`]: reloaded_code_core::ToolError
    fn with_tool(self, tool: &'static str) -> Result<T, AgentBuildError>;
}

impl<Deps> HookedToolExecutor<Deps> {
    pub(crate) fn new<T: Tool<Deps> + 'static>(
        tool: T,
        hooks: &HookSet,
        agent_name: &str,
        tool_name: &'static str,
    ) -> Self
    where
        Deps: Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(ToolAsExecutor(tool)),
            hooks: hooks.clone(),
            agent_name: agent_name.into(),
            tool_name,
        }
    }

    pub(crate) fn from_dyn(
        executor: Box<dyn serdes_ai::agent::ToolExecutor<Deps> + Send + Sync>,
        hooks: &HookSet,
        agent_name: &str,
        tool_name: &'static str,
    ) -> Self {
        Self {
            inner: Arc::from(executor),
            hooks: hooks.clone(),
            agent_name: agent_name.into(),
            tool_name,
        }
    }
}

impl<'a, Deps: Send + Sync + 'static> CoreToolExecutor for CoreToolBridge<'a, Deps> {
    fn execute<'b>(
        &'b self,
        _ctx: &'b ToolCallContext<'b>,
        req: ToolRequest,
    ) -> ToolHookFuture<'b> {
        let inner = self.inner;
        let ctx = self.ctx;
        let captured = self.captured;
        let args = req.args;
        Box::pin(async move {
            match inner.execute(args, ctx).await {
                Ok(tool_return) => {
                    // Hooks see the text projection; the original is restored
                    // after dispatch when the output is untouched.
                    let output = crate::convert::return_to_output(&tool_return);
                    *captured.return_value.lock().expect("capture lock") = Some(tool_return);
                    Ok(output)
                }
                Err(err) => {
                    // Hooks see a core error projection; the original is
                    // restored after dispatch when the error is untouched.
                    let core_err = crate::convert::serdes_error_to_core(&err);
                    *captured.error.lock().expect("capture lock") = Some(err);
                    Err(core_err)
                }
            }
        })
    }
}

/// True when the hook-chain output is byte-identical to the text projection
/// of the original tool return, meaning no hook modified it.
fn output_matches_original(output: &reloaded_code_core::ToolOutput, original: &ToolReturn) -> bool {
    let projection = crate::convert::return_to_output(original);
    output.content == projection.content && output.truncated == projection.truncated
}

#[async_trait]
impl<Deps: Send + Sync + 'static> ToolExecutor<Deps> for DynToolAsExecutor<Deps> {
    async fn execute(
        &self,
        args: JsonValue,
        ctx: &AgentRunContext<Deps>,
    ) -> Result<ToolReturn, ToolError> {
        let tools_ctx = ToolsRunContext::from_arc(ctx.deps.clone(), &ctx.model_name)
            .with_run_id(&ctx.run_id)
            .with_model_settings(ctx.model_settings.clone())
            .with_tool_context(
                ctx.tool_name.as_deref().unwrap_or_default(),
                ctx.tool_call_id.clone(),
            );

        self.0.call(&tools_ctx, args).await
    }
}

// Derived from serdes-ai trait signatures:
//  - [`AgentBuilder::tool_with_executor`] stores a `dyn serdes_ai::agent::ToolExecutor<Deps>`.
//  - [`ToolExecutor::execute`] receives `(args: JsonValue, ctx: &RunContext<Deps>)` and returns `Result<ToolReturn, ToolError>`.
//  - [`HookSet::dispatch_tool`] needs a `dyn reloaded_code_core::hooks::ToolExecutor`,
//    whose [`execute`](reloaded_code_core::hooks::ToolExecutor::execute) takes `(ctx: &ToolCallContext, req: ToolRequest)` and returns a pinned future.
//
// [`HookedToolExecutor`] wraps the stored serdes-ai executor. When hooks are present,
// it adapts args to [`ToolRequest`], borrows the incoming [`AgentRunContext`] by reference,
// and passes a [`CoreToolBridge`] as the core [`ToolExecutor`](reloaded_code_core::hooks::ToolExecutor). The bridge delegates back
// to the original serdes-ai executor inside the hook chain, so hooks can intercept
// or wrap the real tool call.
//
// When no hooks are registered, this passes through directly to `inner.execute(args, ctx)`
// with no extra allocations.
#[async_trait]
impl<Deps: Send + Sync + 'static> serdes_ai::agent::ToolExecutor<Deps>
    for HookedToolExecutor<Deps>
{
    async fn execute(
        &self,
        args: JsonValue,
        ctx: &AgentRunContext<Deps>,
    ) -> Result<ToolReturn, ToolError> {
        if self.hooks.tool_hooks_is_empty() {
            return self.inner.execute(args, ctx).await;
        }
        let tool_ctx = ToolCallContext {
            tool_name: self.tool_name,
            agent_name: &self.agent_name,
            run_id: &ctx.run_id,
        };
        let tool_req = ToolRequest::new(args);
        let captured = CapturedToolResult::default();
        let bridge = CoreToolBridge {
            inner: &*self.inner,
            ctx,
            captured: &captured,
        };
        let result = self.hooks.dispatch_tool(&tool_ctx, tool_req, &bridge).await;
        match result {
            Ok(output) => {
                // Untouched pass-through: hand back the original `ToolReturn`
                // so image content, `tool_call_id`, and exact JSON survive.
                if let Some(original) = captured.return_value.lock().expect("capture lock").take()
                    && output_matches_original(&output, &original)
                {
                    return Ok(original);
                }
                Ok(crate::convert::output_to_return(output))
            }
            Err(core_err) => {
                // Untouched inner failure: hand back the original `ToolError`
                // so structured validation details survive. Hook-produced
                // errors (different message) still convert through the core
                // mapping.
                if let Some(original) = captured.error.lock().expect("capture lock").take()
                    && crate::convert::serdes_error_to_core(&original).to_string()
                        == core_err.to_string()
                {
                    return Err(original);
                }
                Err(crate::convert::core_error_to_serdes(
                    self.tool_name,
                    core_err,
                ))
            }
        }
    }
}

#[async_trait]
impl<Deps: Send + Sync + 'static, T: Tool<Deps>> ToolExecutor<Deps> for ToolAsExecutor<T> {
    async fn execute(
        &self,
        args: JsonValue,
        ctx: &AgentRunContext<Deps>,
    ) -> Result<ToolReturn, ToolError> {
        // Convert agent::RunContext to tools::RunContext
        let tools_ctx = ToolsRunContext::from_arc(ctx.deps.clone(), &ctx.model_name)
            .with_run_id(&ctx.run_id)
            .with_model_settings(ctx.model_settings.clone())
            .with_tool_context(
                ctx.tool_name.as_deref().unwrap_or_default(),
                ctx.tool_call_id.clone(),
            );

        self.0.call(&tools_ctx, args).await
    }
}

impl<Deps, Output> AgentBuilderExt<Deps, Output> for AgentBuilder<Deps, Output>
where
    Deps: Send + Sync + 'static,
    Output: Send + Sync + 'static,
{
    fn tool<T: Tool<Deps> + 'static>(self, tool: T) -> Self {
        let definition = tool.definition();
        self.tool_with_executor(definition, ToolAsExecutor(tool))
    }

    fn tool_dyn(
        self,
        definition: serdes_ai::ToolDefinition,
        tool: Box<dyn Tool<Deps> + Send + Sync>,
    ) -> Self {
        self.tool_with_executor(definition, DynToolAsExecutor(tool))
    }
}

impl<T> ToolResultExt<T> for Result<T, reloaded_code_core::ToolError> {
    /// # Errors
    /// - Returns [`AgentBuildError::ToolSettingsValidation`] when the original result
    ///   contains a [`ToolError`], preserving the tool name and original error.
    fn with_tool(self, tool: &'static str) -> Result<T, AgentBuildError> {
        self.map_err(|source| AgentBuildError::ToolSettingsValidation { tool, source })
    }
}
