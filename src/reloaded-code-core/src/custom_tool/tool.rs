//! Portable custom tool trait.

use super::{CustomToolDefinition, ToolRunContext};
use crate::context::ToolContext;
use crate::{ToolOutput, ToolResult};
use std::future::Future;
use std::pin::Pin;

/// Boxed future returned by [`CustomTool::call`].
pub type CustomToolFuture<'a> = Pin<Box<dyn Future<Output = ToolResult<ToolOutput>> + Send + 'a>>;

/// Framework-neutral custom tool implementation.
///
/// Implement this once for a custom tool, then let framework adapters wrap the
/// trait object in their native tool trait. This keeps the tool definition,
/// prompt guidance, argument schema, and execution logic portable.
pub trait CustomTool: ToolContext + Send + Sync + 'static {
    /// Returns the model-facing definition for this tool.
    #[must_use]
    fn definition(&self) -> CustomToolDefinition;

    /// Executes the tool with JSON arguments from the model.
    ///
    /// The returned [`ToolOutput`] is framework-neutral; adapters convert it to
    /// the native return type expected by their LLM framework.
    ///
    /// # Errors
    ///
    /// Returns a [`ToolError`]. Common call failures include:
    ///
    /// - [`ToolError::Validation`] for malformed arguments.
    /// - [`ToolError::Io`] on filesystem failures.
    /// - [`ToolError::Execution`] for command execution failures.
    /// - [`ToolError::Http`] for network request failures.
    /// - [`ToolError::Json`] for serialization or deserialization failures.
    /// - [`ToolError::Timeout`] and [`ToolError::TimeoutWithKillFailure`]
    ///   when execution time limits are exceeded.
    /// - [`ToolError::PermissionDenied`] when access to the requested
    ///   resource is denied.
    ///
    /// See [`ToolError`] for additional variants.
    ///
    /// [`ToolError`]: crate::ToolError
    /// [`ToolError::Validation`]: crate::ToolError::Validation
    /// [`ToolError::Io`]: crate::ToolError::Io
    /// [`ToolError::Execution`]: crate::ToolError::Execution
    /// [`ToolError::Http`]: crate::ToolError::Http
    /// [`ToolError::Json`]: crate::ToolError::Json
    /// [`ToolError::Timeout`]: crate::ToolError::Timeout
    /// [`ToolError::TimeoutWithKillFailure`]: crate::ToolError::TimeoutWithKillFailure
    /// [`ToolError::PermissionDenied`]: crate::ToolError::PermissionDenied
    fn call<'a>(&'a self, ctx: ToolRunContext<'a>, args: serde_json::Value)
        -> CustomToolFuture<'a>;
}
