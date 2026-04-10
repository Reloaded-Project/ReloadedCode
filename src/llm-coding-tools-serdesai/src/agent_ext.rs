//! Extension traits for integrating tools with serdes-ai AgentBuilder.
//!
//! This module provides adapters to use [`Tool`] implementations with
//! serdes-ai's [`AgentBuilder`].
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_serdesai::{ReadTool, GlobTool, AbsolutePathResolver};
//! use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
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
    /// use llm_coding_tools_serdesai::{ReadTool, GlobTool, AbsolutePathResolver};
    /// use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
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

/// Extension for converting [`ToolError`] results into [`AgentBuildError`].
///
/// This avoids repeating the full `ToolSettingsValidation` struct literal at
/// every `.map_err` call site.
///
/// # Example
///
/// ```no_run
/// use llm_coding_tools_serdesai::agent_ext::ToolResultExt;
/// # use llm_coding_tools_serdesai::AgentBuildError;
/// # fn demo(r: Result<usize, llm_coding_tools_core::ToolError>) -> Result<(), AgentBuildError> {
/// let value = r.with_tool("my_tool")?;
/// # Ok(())
/// # }
/// ```
pub trait ToolResultExt<T> {
    /// Maps a [`ToolError`](llm_coding_tools_core::ToolError) to
    /// [`AgentBuildError::ToolSettingsValidation`].
    ///
    /// # Errors
    /// - Returns [`AgentBuildError::ToolSettingsValidation`] when the original result
    ///   contains a [`ToolError`], preserving the tool name and original error.
    fn with_tool(self, tool: &'static str) -> Result<T, AgentBuildError>;
}

impl<T> ToolResultExt<T> for Result<T, llm_coding_tools_core::ToolError> {
    /// # Errors
    /// - Returns [`AgentBuildError::ToolSettingsValidation`] when the original result
    ///   contains a [`ToolError`], preserving the tool name and original error.
    fn with_tool(self, tool: &'static str) -> Result<T, AgentBuildError> {
        self.map_err(|source| AgentBuildError::ToolSettingsValidation { tool, source })
    }
}
