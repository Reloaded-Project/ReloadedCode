//! Trait and context for creating tools at build time.

use crate::context::ToolContext;
use crate::tool_context::ToolBuildContext;
use std::any::Any;

/// Build-time factory for a user-defined tool.
///
/// Implement this to register a tool. The same type also acts as [`ToolContext`],
/// supplying the tool's identity and prompt.
///
/// The [`ToolFactory::create()`] method returns a type-erased boxed value.
/// Adapter crates downcast it to the framework-specific tool trait they expect.
///
/// # Example
///
/// ```rust
/// use reloaded_code_core::{ToolBuildContext, ToolFactory};
/// use reloaded_code_core::context::{ToolContext, ToolPrompt};
/// use std::any::Any;
///
/// struct WebSearchFactory;
///
/// struct WebSearchTool;
/// impl WebSearchTool {
///     fn new(_ctx: &ToolBuildContext) -> Self { Self }
/// }
///
/// impl ToolContext for WebSearchFactory {
///     fn name(&self) -> &'static str { "web_search" }
///     fn context(&self) -> ToolPrompt {
///         ToolPrompt::Static("Use web_search to find information online.")
///     }
/// }
///
/// impl ToolFactory for WebSearchFactory {
///     fn create(&self, ctx: &ToolBuildContext) -> Box<dyn Any + Send + Sync> {
///         Box::new(WebSearchTool::new(ctx))
///     }
/// }
/// ```
pub trait ToolFactory: ToolContext + Send + Sync + 'static {
    /// Creates a tool from build-time context.
    ///
    /// Return a [`Box<dyn Any + Send + Sync>`] wrapping the concrete tool value
    /// or framework-specific boxed trait object. Adapter crates decide the
    /// expected downcast type.
    fn create(&self, ctx: &ToolBuildContext) -> Box<dyn Any + Send + Sync>;
}
