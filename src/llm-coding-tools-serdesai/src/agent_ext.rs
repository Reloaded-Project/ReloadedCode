//! Extension traits for integrating tools with serdes-ai AgentBuilder.
//!
//! This module provides adapters to use [`Tool`] implementations with
//! serdes-ai's [`AgentBuilder`].
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_serdesai::absolute::{ReadTool, GlobTool};
//! use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
//! use serdes_ai::prelude::*;
//!
//! # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! let agent = AgentBuilder::<(), String>::from_model("openai:gpt-4o")?
//!     .tool(ReadTool::<true>::new())
//!     .tool(GlobTool::new())
//!     .system_prompt("You are helpful.")
//!     .build();
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use serdes_ai::agent::ToolExecutor;
use serdes_ai::tools::{RunContext as ToolsRunContext, Tool, ToolError, ToolReturn};
use serdes_ai::{AgentBuilder, RunContext as AgentRunContext};

/// Adapter that wraps a [`Tool`] to implement [`ToolExecutor`].
///
/// This bridges the gap between `serdes_ai::tools::Tool` (which uses
/// `tools::RunContext`) and `serdes_ai::agent::ToolExecutor` (which uses
/// `agent::RunContext`).
struct ToolAsExecutor<T>(T);

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
            .with_model_settings(ctx.model_settings.clone());

        self.0.call(&tools_ctx, args).await
    }
}

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
    /// use llm_coding_tools_serdesai::absolute::{ReadTool, GlobTool};
    /// use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
    /// use serdes_ai::prelude::*;
    ///
    /// # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// let agent = AgentBuilder::<(), String>::from_model("openai:gpt-4o")?
    ///     .tool(ReadTool::<true>::new())
    ///     .tool(GlobTool::new())
    ///     .build();
    /// # Ok(())
    /// # }
    /// ```
    fn tool<T: Tool<Deps> + 'static>(self, tool: T) -> Self;
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
}
