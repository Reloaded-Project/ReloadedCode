//! Trait and context for creating tools at build time.

use super::CustomTool;
use crate::context::ToolContext;
use crate::tool_context::ToolBuildContext;
use crate::ToolResult;
use std::sync::Arc;

/// Build-time factory for a user-defined tool.
///
/// Implement this to register a tool. The same type also acts as [`ToolContext`],
/// supplying the tool's identity and prompt.
///
/// The [`ToolFactory::create()`] method returns a portable custom tool trait
/// object. Adapter crates wrap that object in the framework-specific tool trait
/// they expect.
///
/// For a complete example that implements [`ToolContext`], [`CustomTool`], and
/// [`ToolFactory`], then registers the factory with
/// [`CustomToolRegistry`](super::CustomToolRegistry), see the
/// [`custom_tool`](super) module documentation.
pub trait ToolFactory: ToolContext + Send + Sync + 'static {
    /// Creates a tool from build-time context.
    ///
    /// Return a portable [`CustomTool`] trait object. Adapter crates wrap the
    /// object in their native framework-specific tool type.
    ///
    /// # Errors
    /// Returns a [`ToolError`](crate::ToolError) when constructing the tool fails.
    fn create(&self, ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>>;
}
