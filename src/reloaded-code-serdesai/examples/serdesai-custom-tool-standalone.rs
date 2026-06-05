//! Standalone portable custom tool example (no AgentRuntime).
//!
//! Demonstrates using a portable `CustomTool` with a plain SerdesAI agent
//! builder, without the agent catalog / runtime infrastructure.
//!
//! The custom tool depends only on `reloaded-code-core`. The SerdesAI
//! [`CustomToolAdapter`] wraps it so it can be attached via
//! [`AgentBuilderExt::tool`].
//!
//! Run: Edit the `API_KEY` constant below, or set the `OPENAI_API_KEY`
//! environment variable, then:
//!      cargo run --example serdesai-custom-tool-standalone -p reloaded-code-serdesai

use futures::StreamExt;
use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::{
    CustomTool, CustomToolDefinition, CustomToolFuture, ToolOutput, ToolRunContext,
};
use reloaded_code_serdesai::SystemPromptBuilder;
use reloaded_code_serdesai::agent_ext::AgentBuilderExt;
use reloaded_code_serdesai::tools::CustomToolAdapter;
use serde_json::json;
use serdes_ai::prelude::*;
use serdes_ai_models::OpenAIChatModel;
use std::fmt::Write;
use std::sync::Arc;

const MODEL_ID: &str = "hf:zai-org/GLM-4.7-Flash";
const BASE_URL: &str = "https://api.synthetic.new/openai/v1";
/// Fallback API key if env var is not set. Leave empty to require env var.
const API_KEY: &str = "";

fn get_api_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| API_KEY.to_string())
}

// -- Portable custom tool (depends only on reloaded-code-core) --

struct ProjectInfoTool;

impl ToolContext for ProjectInfoTool {
    fn name(&self) -> &'static str {
        "project_info"
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static("Use project_info to get demo metadata about this host application.")
    }
}

impl CustomTool for ProjectInfoTool {
    fn definition(&self) -> CustomToolDefinition {
        CustomToolDefinition::new(
            "project_info",
            "Returns host-provided metadata about the running environment.",
        )
        .with_parameters(json!({
            "type": "object",
            "properties": {
                "include_cwd": {
                    "type": "boolean",
                    "description": "Include the current working directory."
                }
            },
            "additionalProperties": false
        }))
    }

    fn call<'a>(
        &'a self,
        ctx: ToolRunContext<'a>,
        args: serde_json::Value,
    ) -> CustomToolFuture<'a> {
        Box::pin(async move {
            let include_cwd = args
                .get("include_cwd")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

            let mut lines = vec![
                format!(
                    "called_by_model: {}",
                    ctx.model_name().unwrap_or("<unknown>")
                ),
                format!("run_id_present: {}", ctx.run_id().is_some()),
                format!("tool_call_id_present: {}", ctx.tool_call_id().is_some()),
            ];

            if include_cwd {
                lines.push(format!(
                    "cwd: {}",
                    std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                ));
            }

            Ok(ToolOutput::new(lines.join("\n")))
        })
    }
}

// -- Main --

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let model = OpenAIChatModel::new(MODEL_ID, get_api_key()).with_base_url(BASE_URL);

    let mut pb = SystemPromptBuilder::new()
        .working_directory(std::env::current_dir()?.display().to_string());

    let agent = AgentBuilder::<(), String>::new(model)
        .instructions("Call project_info with include_cwd=true, then summarize in three bullets.")
        .tool(pb.track(CustomToolAdapter::new(Arc::new(ProjectInfoTool))))
        .system_prompt(pb.build())
        .build();

    println!("Agent ready ({} tools).", agent.tools().len());

    let prompt =
        "Call project_info with include_cwd=true, then summarize what it says in three bullets.";
    let mut stream = agent.run_stream(prompt, ()).await?;

    fn log_xml(request_id: u32, tag: &str, content: &str) {
        let mut line = String::with_capacity(content.len() + tag.len() * 2 + 18);
        let _ = write!(line, "<{request_id}:{tag}>{content}</{tag}>");
        println!("{line}");
    }

    let mut request_id = 0u32;
    log_xml(request_id, "user", prompt);
    request_id = request_id.saturating_add(1);
    let mut assistant_message = String::with_capacity(256);

    while let Some(event) = stream.next().await {
        match event? {
            AgentStreamEvent::TextDelta { text, .. } => assistant_message.push_str(&text),
            AgentStreamEvent::RequestStart { .. } => assistant_message.clear(),
            AgentStreamEvent::ToolCallStart { tool_name, .. } => {
                log_xml(request_id, "tool", &tool_name);
                request_id = request_id.saturating_add(1);
            }
            AgentStreamEvent::ResponseComplete { .. } => {
                log_xml(request_id, "assistant", &assistant_message);
                request_id = request_id.saturating_add(1);
            }
            _ => {}
        }
    }

    Ok(())
}
