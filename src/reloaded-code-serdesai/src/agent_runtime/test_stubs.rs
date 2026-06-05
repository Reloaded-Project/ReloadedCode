//! Shared test stubs for SerdesAI custom tool tests.

use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::{
    CustomTool, CustomToolDefinition, CustomToolFuture, ToolBuildContext, ToolFactory, ToolOutput,
    ToolResult, ToolRunContext,
};
use std::sync::Arc;

/// A minimal portable custom tool that returns a configurable text response.
struct SerdesTestTool {
    name: &'static str,
    prompt: &'static str,
    response: &'static str,
}

impl ToolContext for SerdesTestTool {
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(self.prompt)
    }
}

impl CustomTool for SerdesTestTool {
    #[inline]
    fn definition(&self) -> CustomToolDefinition {
        CustomToolDefinition::new(self.name, self.name)
    }

    #[inline]
    fn call<'a>(
        &'a self,
        _ctx: ToolRunContext<'a>,
        _args: serde_json::Value,
    ) -> CustomToolFuture<'a> {
        Box::pin(async move { Ok(ToolOutput::new(self.response)) })
    }
}

/// A `ToolFactory` that creates a portable [`SerdesTestTool`].
///
/// `name` and `prompt` are surfaced via `ToolContext` for system-prompt
/// guidance injection. `response` is returned by the tool's `call()`.
#[derive(Debug)]
pub struct SerdesTestFactory {
    /// Tool name passed to `ToolContext::name()` and `ToolDefinition::new()`.
    pub name: &'static str,
    /// Prompt text passed to `ToolContext::context()`.
    pub prompt: &'static str,
    /// Text returned by `SerdesTestTool::call()`.
    pub response: &'static str,
}

impl SerdesTestFactory {
    /// Creates a new factory that produces a tool named `name`, with system-prompt
    /// guidance `prompt`, and `call()` returning `response`.
    #[inline]
    pub fn new(name: &'static str, prompt: &'static str, response: &'static str) -> Self {
        Self {
            name,
            prompt,
            response,
        }
    }
}

impl ToolContext for SerdesTestFactory {
    #[inline]
    fn name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(self.prompt)
    }
}

impl ToolFactory for SerdesTestFactory {
    #[inline]
    fn create(&self, _ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
        Ok(Arc::new(SerdesTestTool {
            name: self.name,
            prompt: self.prompt,
            response: self.response,
        }))
    }
}
