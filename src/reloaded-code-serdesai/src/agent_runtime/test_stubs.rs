//! Shared test stubs for SerdesAI custom tool tests.
//!
//! These combine a [`serdes_ai::Tool<()>`] implementation with a
//! [`ToolFactory`] so tests can exercise the full agent-build pipeline
//! including tool attachment and prompt guidance injection.

use async_trait::async_trait;
use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::{ToolBuildContext, ToolFactory};
use serdes_ai::tools::{RunContext, ToolDefinition, ToolReturn};
use std::any::Any;

/// A minimal `serdes_ai::Tool<()>` that returns a configurable text response.
struct SerdesTestTool {
    name: &'static str,
    response: &'static str,
}

#[async_trait]
impl serdes_ai::Tool<()> for SerdesTestTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.name, self.name)
    }

    async fn call(&self, _ctx: &RunContext<()>, _args: serde_json::Value) -> serdes_ai::ToolResult {
        Ok(ToolReturn::text(self.response))
    }
}

/// A `ToolFactory` that creates a [`SerdesTestTool`] and returns it as a
/// double-boxed `Box<dyn Any + Send + Sync>`.
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
    fn create(&self, _ctx: &ToolBuildContext) -> Box<dyn Any + Send + Sync> {
        let tool: Box<dyn serdes_ai::Tool<()>> = Box::new(SerdesTestTool {
            name: self.name,
            response: self.response,
        });
        Box::new(tool)
    }
}
