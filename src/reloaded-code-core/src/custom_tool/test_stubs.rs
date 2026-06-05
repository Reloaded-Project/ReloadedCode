//! Shared test stubs for custom tool tests.

use super::{CustomTool, CustomToolDefinition, CustomToolFuture, ToolBuildContext, ToolFactory};
use crate::context::{ToolContext, ToolPrompt};
use crate::{ToolOutput, ToolResult};
use std::sync::Arc;

/// Minimal factory returning a configurable prompt and empty boxed value.
pub(crate) struct TestFactory {
    pub(crate) tool_name: &'static str,
    pub(crate) prompt: &'static str,
}

impl TestFactory {
    pub(crate) fn new(name: &'static str, prompt: &'static str) -> Self {
        Self {
            tool_name: name,
            prompt,
        }
    }
}

impl ToolContext for TestFactory {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(self.prompt)
    }
}

impl ToolFactory for TestFactory {
    fn create(&self, _ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
        Ok(Arc::new(TestTool {
            tool_name: self.tool_name,
            prompt: self.prompt,
        }))
    }
}

/// Factory that returns a portable echo tool for registry tests.
pub(crate) struct EchoFactory {
    /// Tool name passed to [`ToolContext::name`].
    pub(crate) tool_name: &'static str,
}

impl EchoFactory {
    /// Creates a new [`EchoFactory`] with the given tool name.
    pub(crate) fn new(name: &'static str) -> Self {
        Self { tool_name: name }
    }
}

impl ToolContext for EchoFactory {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static("echo tool prompt")
    }
}

impl ToolFactory for EchoFactory {
    fn create(&self, _ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
        Ok(Arc::new(TestTool {
            tool_name: self.tool_name,
            prompt: "echo tool prompt",
        }))
    }
}

/// Minimal portable custom tool used by factories above.
struct TestTool {
    tool_name: &'static str,
    prompt: &'static str,
}

impl ToolContext for TestTool {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(self.prompt)
    }
}

impl CustomTool for TestTool {
    fn definition(&self) -> CustomToolDefinition {
        CustomToolDefinition::new(self.tool_name, "test custom tool")
    }

    fn call<'a>(
        &'a self,
        _ctx: super::ToolRunContext<'a>,
        _args: serde_json::Value,
    ) -> CustomToolFuture<'a> {
        Box::pin(async { Ok(ToolOutput::new("ok")) })
    }
}
