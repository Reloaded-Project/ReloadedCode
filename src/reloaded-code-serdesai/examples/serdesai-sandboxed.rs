//! Sandboxed tools example - restricted file access.
//!
//! Demonstrates using `allowed` tools that restrict file operations
//! to specific directories only. This is useful for:
//!
//! - Multi-tenant environments where agents should only access their workspace
//! - Security-conscious deployments limiting filesystem exposure
//! - Project-scoped agents that shouldn't touch system files
//!
//! Run: cargo run --example serdesai-sandboxed -p reloaded-code-serdesai

use futures::StreamExt;
use reloaded_code_serdesai::AllowedPathResolver;
use reloaded_code_serdesai::SystemPromptBuilder;
use reloaded_code_serdesai::agent_ext::AgentBuilderExt;
use reloaded_code_serdesai::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};
use serdes_ai::prelude::*;
use serdes_ai_models::OpenAIChatModel;
use std::fmt::Write;

// Set your OpenAI API key here or via OPENAI_API_KEY environment variable.
/// Fallback API key if env var is not set. Leave empty to require env var.
const OPENAI_API_KEY: &str = "";
const OPENAI_MODEL: &str = "hf:zai-org/GLM-4.7-Flash";
const OPENAI_BASE_URL: &str = "https://api.synthetic.new/openai/v1";

fn get_openai_api_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| OPENAI_API_KEY.to_string())
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // === Define allowed directories ===
    //
    // Only these directories (and their subdirectories) will be accessible.
    // Attempts to read/write outside these paths will fail with an error.
    let allowed_paths = vec![
        std::env::current_dir()?, // Current working directory
        std::env::temp_dir(),     // Temp directory (cross-platform)
    ];

    // === Create resolver and tools ===
    //
    // Create one resolver and share it across tools.
    // More efficient and ensures consistency.
    let resolver = AllowedPathResolver::new(allowed_paths)?;

    let read = ReadTool::new(resolver.clone());
    let write = WriteTool::new(resolver.clone());
    let edit = EditTool::new(resolver.clone());
    let glob = GlobTool::new(resolver.clone());
    let grep = GrepTool::new(resolver.clone());

    // === Build agent with sandboxed tools ===
    //
    // Use SystemPromptBuilder with fluent chaining:
    // - working_directory() and allowed_paths() consume self (chaining)
    // - track() takes &mut self (passthrough for agent builder)
    let mut pb = SystemPromptBuilder::new()
        .working_directory(std::env::current_dir()?.display().to_string())
        .allowed_paths(&resolver);

    let model =
        OpenAIChatModel::new(OPENAI_MODEL, get_openai_api_key()).with_base_url(OPENAI_BASE_URL);
    let agent = AgentBuilder::<(), String>::new(model)
        .instructions("Use tools to answer; call at least one tool before responding.")
        .tool(pb.track(read))
        .tool(pb.track(write))
        .tool(pb.track(edit))
        .tool(pb.track(glob))
        .tool(pb.track(grep))
        .system_prompt(pb.build())
        .build();

    // === Print info ===
    println!(
        "=== Sandboxed Agent Ready ({} tools) ===",
        agent.tools().len()
    );
    println!("Allowed paths:");
    println!("  - Current directory: {:?}", std::env::current_dir()?);
    println!("  - Temp directory: {:?}", std::env::temp_dir());

    // === Run the agent ===
    println!("\n=== Running Agent ===");
    let prompt = "List the Rust files in the current directory using glob";
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
