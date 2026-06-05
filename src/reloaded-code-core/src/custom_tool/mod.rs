//! Custom tool registration primitives.
//!
//! Embedders implement [`CustomTool`] and [`ToolFactory`] to provide portable
//! custom tools that integrate with framework adapters, permission rules, and
//! system prompt builders without depending on a specific LLM framework.
//!
//! # Public API
//!
//! - [`CustomTool`] - Framework-neutral trait for tool definition and execution.
//! - [`CustomToolDefinition`] - Framework-neutral name, description, and schema.
//! - [`ToolRunContext`] - Optional framework metadata passed to tool calls.
//! - [`ToolFactory`] - Trait for creating custom tools at build time. Extends
//!   [`ToolContext`](crate::ToolContext) so factories provide name and prompt
//!   guidance the same way built-in tools do.
//! - [`ToolBuildContext`] - Context passed to [`ToolFactory::create`]. Built-in
//!   tools get this too, plus whatever extra dependencies they need.
//! - [`CustomToolRegistry`] - Registry of custom tool factories.
//! - [`SharedToolRegistry`] - Shared wrapper around a registry for cheap cloning.
//!
//! # Usage
//!
//! ```rust
//! use reloaded_code_core::{CustomTool, CustomToolDefinition, CustomToolFuture, CustomToolRegistry, ToolBuildContext, ToolFactory, ToolOutput, ToolResult, ToolRunContext};
//! use reloaded_code_core::context::{ToolContext, ToolPrompt};
//! use serde_json::json;
//! use std::sync::Arc;
//!
//! struct MyFactory;
//!
//! struct MyTool;
//! impl MyTool {
//!     fn new(_ctx: &ToolBuildContext) -> Self { Self }
//! }
//!
//! impl ToolContext for MyFactory {
//!     fn name(&self) -> &'static str { "my_tool" }
//!     fn context(&self) -> ToolPrompt {
//!         ToolPrompt::Static("Use my_tool to do things.")
//!     }
//! }
//!
//! impl ToolContext for MyTool {
//!     fn name(&self) -> &'static str { "my_tool" }
//!     fn context(&self) -> ToolPrompt {
//!         ToolPrompt::Static("Use my_tool to do things.")
//!     }
//! }
//!
//! impl CustomTool for MyTool {
//!     fn definition(&self) -> CustomToolDefinition {
//!         CustomToolDefinition::new("my_tool", "Does things")
//!             .with_parameters(json!({
//!                 "type": "object",
//!                 "properties": {
//!                     "query": { "type": "string", "description": "Search query" }
//!                 },
//!                 "required": ["query"]
//!             }))
//!     }
//!
//!     fn call<'a>(&'a self, _ctx: ToolRunContext<'a>, args: serde_json::Value) -> CustomToolFuture<'a> {
//!         Box::pin(async move {
//!             let query = args["query"].as_str().unwrap_or_default();
//!             Ok(ToolOutput::new(format!("searched for {query}")))
//!         })
//!     }
//! }
//!
//! impl ToolFactory for MyFactory {
//!     fn create(&self, ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
//!         Ok(Arc::new(MyTool::new(ctx)))
//!     }
//! }
//!
//! let mut registry = CustomToolRegistry::new();
//! registry.insert(MyFactory);
//! assert!(registry.get("my_tool").is_some());
//! ```

pub(crate) mod definition;
pub(crate) mod factory;
pub(crate) mod registry;
pub(crate) mod runtime;
pub(crate) mod tool;

pub use crate::tool_context::ToolBuildContext;
pub use definition::CustomToolDefinition;
pub use factory::ToolFactory;
pub use registry::{CustomToolRegistry, SharedToolRegistry};
pub use runtime::ToolRunContext;
pub use tool::{CustomTool, CustomToolFuture};

#[cfg(test)]
pub(crate) mod test_stubs;

#[cfg(test)]
mod tests {
    use super::test_stubs::{EchoFactory, TestFactory};
    use super::*;
    use crate::context::ToolContext;
    use crate::context::ToolPrompt;

    #[test]
    fn registry_inserts_and_retrieves_factory() {
        let mut registry = CustomToolRegistry::new();
        assert!(registry.is_empty());

        registry.insert(EchoFactory::new("echo"));
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());

        let factory = registry.get("echo").expect("factory should exist");
        assert_eq!(factory.name(), "echo");
    }

    #[test]
    fn registry_returns_none_for_unknown_name() {
        let registry = CustomToolRegistry::new();
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn registry_insert_replaces_existing() {
        let mut registry = CustomToolRegistry::new();
        registry.insert(EchoFactory::new("tool_a"));
        registry.insert(EchoFactory::new("tool_a"));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn factory_create_returns_portable_tool() {
        let factory = EchoFactory::new("echo");
        let ctx = ToolBuildContext::new(std::path::Path::new("/tmp"), None).unwrap();
        let tool = factory.create(&ctx).expect("factory should create tool");

        assert_eq!(tool.name(), "echo");
        assert_eq!(tool.definition().name, "echo");
    }

    #[test]
    fn factory_context_returns_prompt() {
        let factory = EchoFactory::new("echo");
        assert!(matches!(factory.context(), ToolPrompt::Static(_)));
    }

    #[test]
    fn factory_context_can_skip_guidance_with_empty_static_prompt() {
        let factory = TestFactory::new("no_context", "");
        assert!(matches!(factory.context(), ToolPrompt::Static("")));
    }

    #[test]
    fn shared_registry_clones_and_accesses_factories() {
        let mut registry = CustomToolRegistry::new();
        registry.insert(EchoFactory::new("echo"));
        let shared = SharedToolRegistry::from_registry(registry);

        let cloned = shared.clone();
        assert!(cloned.get("echo").is_some());
        assert_eq!(cloned.len(), 1);
    }
}
