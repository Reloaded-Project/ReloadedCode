//! Runtime types for portable custom tool calls.

/// Framework-neutral metadata available when a custom tool is called.
///
/// Framework adapters populate whichever fields their runtime exposes. Custom
/// tools should treat every field as optional so the same implementation can run
/// across adapters with different context capabilities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolRunContext<'a> {
    model_name: Option<&'a str>,
    run_id: Option<&'a str>,
    tool_call_id: Option<&'a str>,
}

impl<'a> ToolRunContext<'a> {
    /// Creates an empty context.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            model_name: None,
            run_id: None,
            tool_call_id: None,
        }
    }

    /// Adds the model name supplied by the framework, if any.
    #[must_use]
    pub const fn with_model_name(mut self, model_name: &'a str) -> Self {
        self.model_name = Some(model_name);
        self
    }

    /// Adds the framework run identifier, if any.
    #[must_use]
    pub const fn with_run_id(mut self, run_id: &'a str) -> Self {
        self.run_id = Some(run_id);
        self
    }

    /// Adds the framework tool-call identifier, if any.
    #[must_use]
    pub const fn with_tool_call_id(mut self, tool_call_id: &'a str) -> Self {
        self.tool_call_id = Some(tool_call_id);
        self
    }

    /// Returns the model name supplied by the framework, if any.
    #[must_use]
    pub const fn model_name(&self) -> Option<&'a str> {
        self.model_name
    }

    /// Returns the framework run identifier, if any.
    #[must_use]
    pub const fn run_id(&self) -> Option<&'a str> {
        self.run_id
    }

    /// Returns the framework tool-call identifier, if any.
    #[must_use]
    pub const fn tool_call_id(&self) -> Option<&'a str> {
        self.tool_call_id
    }
}

#[cfg(test)]
mod tests {
    use super::ToolRunContext;

    #[test]
    fn context_starts_empty() {
        let ctx = ToolRunContext::new();

        assert_eq!(ctx.model_name(), None);
        assert_eq!(ctx.run_id(), None);
        assert_eq!(ctx.tool_call_id(), None);
    }

    #[test]
    fn context_records_framework_metadata() {
        let ctx = ToolRunContext::new()
            .with_model_name("model")
            .with_run_id("run")
            .with_tool_call_id("call");

        assert_eq!(ctx.model_name(), Some("model"));
        assert_eq!(ctx.run_id(), Some("run"));
        assert_eq!(ctx.tool_call_id(), Some("call"));
    }
}
