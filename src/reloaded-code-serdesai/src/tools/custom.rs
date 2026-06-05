//! SerdesAI adapter for portable core custom tools.
//!
//! Wraps an [`Arc<dyn CustomTool>`] so it can be used directly with SerdesAI's
//! [`AgentBuilder`](serdes_ai::AgentBuilder) via
//! [`AgentBuilderExt`](crate::agent_ext::AgentBuilderExt).
//!
//! The adapter is also used internally by the agent-runtime build layer, but
//! it lives here so non-agent users can attach portable custom tools to a plain
//! SerdesAI agent without going through [`AgentRuntimeBuilder`].
//!
//! [`AgentRuntimeBuilder`]: reloaded_code_agents::AgentRuntimeBuilder
//! [`AgentBuilderExt`]: crate::agent_ext::AgentBuilderExt
//!
//! # Example
//!
//! ```no_run
//! use reloaded_code_core::{
//!     CustomTool, CustomToolDefinition, CustomToolFuture, ToolOutput, ToolResult,
//!     ToolRunContext, context::{ToolContext, ToolPrompt},
//! };
//! use reloaded_code_serdesai::tools::CustomToolAdapter;
//! use reloaded_code_serdesai::agent_ext::AgentBuilderExt;
//! use reloaded_code_serdesai::SystemPromptBuilder;
//! use serdes_ai::prelude::*;
//! use serde_json::json;
//! use std::sync::Arc;
//!
//! // 1. Implement the portable custom tool (depends only on reloaded-code-core)
//! struct EchoTool;
//!
//! impl ToolContext for EchoTool {
//!     fn name(&self) -> &'static str { "echo" }
//!     fn context(&self) -> ToolPrompt {
//!         ToolPrompt::Static("Use echo to repeat a message.")
//!     }
//! }
//!
//! impl CustomTool for EchoTool {
//!     fn definition(&self) -> CustomToolDefinition {
//!         CustomToolDefinition::new("echo", "Echo a message back")
//!             .with_parameters(json!({
//!                 "type": "object",
//!                 "properties": {
//!                     "message": { "type": "string", "description": "Message to echo" }
//!                 },
//!                 "required": ["message"]
//!             }))
//!     }
//!
//!     fn call<'a>(&'a self, _ctx: ToolRunContext<'a>, args: serde_json::Value) -> CustomToolFuture<'a> {
//!         Box::pin(async move {
//!             let msg = args["message"].as_str().unwrap_or_default();
//!             Ok(ToolOutput::new(msg))
//!         })
//!     }
//! }
//!
//! // 2. Wrap with CustomToolAdapter and attach to a SerdesAI agent
//! let mut pb = SystemPromptBuilder::new();
//! let agent = AgentBuilder::<(), String>::from_model("openai:gpt-5.4")?
//!     .tool(pb.track(CustomToolAdapter::new(Arc::new(EchoTool))))
//!     .system_prompt(pb.build())
//!     .build();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use async_trait::async_trait;
use reloaded_code_core::context::ToolContext;
use reloaded_code_core::{CustomTool, ToolRunContext};
use serdes_ai::tools::{RunContext, Tool, ToolDefinition};
use std::sync::Arc;

/// SerdesAI adapter for portable core custom tools.
///
/// Wraps a [`CustomTool`] trait object and implements the SerdesAI [`Tool`]
/// trait so it can be registered via
/// [`AgentBuilderExt::tool`](crate::agent_ext::AgentBuilderExt::tool) or the
/// dynamic [`AgentBuilderExt::tool_dyn`](crate::agent_ext::AgentBuilderExt::tool_dyn)
/// method.
///
/// See the [module-level documentation](crate::tools) for related tool types.
///
/// [`AgentBuilderExt`]: crate::agent_ext::AgentBuilderExt
pub struct CustomToolAdapter {
    inner: Arc<dyn CustomTool>,
}

impl CustomToolAdapter {
    /// Creates a new adapter wrapping the given portable custom tool.
    #[inline]
    pub fn new(inner: Arc<dyn CustomTool>) -> Self {
        Self { inner }
    }
}

impl ToolContext for CustomToolAdapter {
    #[inline]
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    #[inline]
    fn context(&self) -> reloaded_code_core::context::ToolPrompt {
        self.inner.context()
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for CustomToolAdapter {
    fn definition(&self) -> ToolDefinition {
        crate::convert::custom_definition_to_serdes(self.inner.definition())
    }

    async fn call(&self, ctx: &RunContext<Deps>, args: serde_json::Value) -> serdes_ai::ToolResult {
        // Translate SerdesAI's RunContext into the framework-neutral ToolRunContext
        // expected by the inner portable custom tool.
        let mut run_ctx = ToolRunContext::new()
            .with_model_name(ctx.model_name.as_str())
            .with_run_id(ctx.run_id.as_str());
        if let Some(tool_call_id) = ctx.tool_call_id.as_deref() {
            run_ctx = run_ctx.with_tool_call_id(tool_call_id);
        }

        crate::convert::to_serdes_result(self.inner.name(), self.inner.call(run_ctx, args).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reloaded_code_core::context::{ToolContext, ToolPrompt};
    use reloaded_code_core::{CustomToolDefinition, CustomToolFuture, ToolOutput};
    use serde_json::json;
    use serdes_ai::tools::RunContext as ToolsRunContext;

    /// Minimal portable tool for testing the adapter.
    struct EchoTool;

    impl ToolContext for EchoTool {
        fn name(&self) -> &'static str {
            "test_echo"
        }
        fn context(&self) -> ToolPrompt {
            ToolPrompt::Static("Echo tool for tests.")
        }
    }

    impl CustomTool for EchoTool {
        fn definition(&self) -> CustomToolDefinition {
            CustomToolDefinition::new("test_echo", "Echoes input")
                .with_parameters(json!({
                    "type": "object",
                    "properties": {
                        "msg": { "type": "string" }
                    }
                }))
                .with_strict(true)
        }

        fn call<'a>(
            &'a self,
            ctx: ToolRunContext<'a>,
            args: serde_json::Value,
        ) -> CustomToolFuture<'a> {
            Box::pin(async move {
                let msg = args["msg"].as_str().unwrap_or_default();
                let model = ctx.model_name().unwrap_or("unknown");
                Ok(ToolOutput::new(format!("{model}:{msg}")))
            })
        }
    }

    #[test]
    fn adapter_exposes_name_and_context_from_inner() {
        let adapter = CustomToolAdapter::new(Arc::new(EchoTool));
        assert_eq!(ToolContext::name(&adapter), "test_echo");
        assert!(matches!(adapter.context(), ToolPrompt::Static(_)));
    }

    #[test]
    fn adapter_converts_definition() {
        let adapter = CustomToolAdapter::new(Arc::new(EchoTool));
        let def = <CustomToolAdapter as serdes_ai::Tool<()>>::definition(&adapter);
        assert_eq!(def.name, "test_echo");
        assert_eq!(def.description, "Echoes input");
        assert_eq!(def.strict, Some(true));
        assert!(def.parameters_json_schema["properties"]["msg"].is_object());
    }

    #[tokio::test]
    async fn adapter_forwards_call_and_context() {
        let adapter = CustomToolAdapter::new(Arc::new(EchoTool));
        let ctx = ToolsRunContext::new((), "test-model");
        let args = json!({"msg": "hello"});

        let result = adapter.call(&ctx, args).await;
        let ret = result.expect("call should succeed");
        assert_eq!(ret.as_text(), Some("test-model:hello"));
    }

    #[tokio::test]
    async fn adapter_propagates_tool_call_id() {
        struct CtxInspector;
        impl ToolContext for CtxInspector {
            fn name(&self) -> &'static str {
                "inspector"
            }
            fn context(&self) -> ToolPrompt {
                ToolPrompt::Static("")
            }
        }
        impl CustomTool for CtxInspector {
            fn definition(&self) -> CustomToolDefinition {
                CustomToolDefinition::new("inspector", "inspector")
            }
            fn call<'a>(
                &'a self,
                ctx: ToolRunContext<'a>,
                _args: serde_json::Value,
            ) -> CustomToolFuture<'a> {
                Box::pin(async move {
                    let id = ctx.tool_call_id().unwrap_or("none");
                    Ok(ToolOutput::new(id.to_string()))
                })
            }
        }

        let adapter = CustomToolAdapter::new(Arc::new(CtxInspector));
        let ctx = ToolsRunContext::new((), "m")
            .with_tool_context("inspector", Some("call-42".to_string()));
        let result = adapter.call(&ctx, json!({})).await;
        let ret = result.expect("should succeed");
        assert_eq!(ret.as_text(), Some("call-42"));
    }

    #[tokio::test]
    async fn adapter_maps_errors() {
        struct FailTool;
        impl ToolContext for FailTool {
            fn name(&self) -> &'static str {
                "fail"
            }
            fn context(&self) -> ToolPrompt {
                ToolPrompt::Static("")
            }
        }
        impl CustomTool for FailTool {
            fn definition(&self) -> CustomToolDefinition {
                CustomToolDefinition::new("fail", "always fails")
            }
            fn call<'a>(
                &'a self,
                _ctx: ToolRunContext<'a>,
                _args: serde_json::Value,
            ) -> CustomToolFuture<'a> {
                Box::pin(async { Err(reloaded_code_core::ToolError::Execution("boom".into())) })
            }
        }

        let adapter = CustomToolAdapter::new(Arc::new(FailTool));
        let ctx = ToolsRunContext::new((), "m");
        let result = adapter.call(&ctx, json!({})).await;
        assert!(result.is_err());
    }
}
