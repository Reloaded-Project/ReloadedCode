//! Shared test stubs for custom tool tests.

use super::{ToolBuildContext, ToolFactory};
use crate::context::{ToolContext, ToolPrompt};
use std::any::Any;

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
    fn create(&self, _ctx: &ToolBuildContext) -> Box<dyn Any + Send + Sync> {
        Box::new(())
    }
}

/// Factory that returns a downcastable integer for registry tests.
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
    fn create(&self, _ctx: &ToolBuildContext) -> Box<dyn Any + Send + Sync> {
        Box::new(42_usize)
    }
}
