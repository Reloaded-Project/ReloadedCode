//! Agent-driven Task tool example (serdesAI).
//!
//! Demonstrates:
//! - Loading a subagent config from an embedded file using include_str!
//! - Using default_tools to build the tool catalog
//! - Building an AgentRegistry from AgentCatalog and tools
//! - Creating a TaskTool for subagent invocation
//! - Setting up a primary agent with only the Task tool (forces delegation)
//! - Running a task that requires the primary agent to invoke a subagent
//! - Streaming output with XML-style logging
//!
//! Run: cargo run --example serdesai-agents -p llm-coding-tools-serdesai

use futures::StreamExt;
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, PermissionAction, Rule, Ruleset};
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, AllowedPathResolver, SystemPromptBuilder, TaskTool,
    TodoState, default_tools,
};
use serdes_ai::agent::ModelConfig;
use serdes_ai::prelude::*;
use std::fmt::Write;
use std::sync::Arc;

// Set your OpenAI API key here or via OPENAI_API_KEY environment variable.
const OPENAI_MODEL: &str = "openai:hf:zai-org/GLM-4.7";
const OPENAI_BASE_URL: &str = "https://api.synthetic.new/openai/v1";

fn get_openai_api_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_default()
}

// Embedded subagent config (loaded via include_str!)
const SUBAGENT_CONFIG: &str = include_str!("agents/serdesai-agents.md");

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // === Load agent config ===
    //
    // Load a single embedded agent config using include_str!.
    let loader = AgentLoader::new();
    let mut catalog = AgentCatalog::new();
    loader.add_from_str(&mut catalog, SUBAGENT_CONFIG, "file-reader")?;

    // === Choose absolute vs allowed tool flow ===
    //
    // Set OPENCODE_USE_ALLOWED environment variable to enable sandboxed (allowed) tools.
    // Without the env var, tools use absolute paths with no restrictions.
    let use_allowed = std::env::var("OPENCODE_USE_ALLOWED").is_ok();
    let resolver = if use_allowed {
        Some(AllowedPathResolver::new([
            std::env::current_dir()?,
            std::env::temp_dir(),
        ])?)
    } else {
        None
    };

    // === Build tool catalog ===
    //
    // Use default_tools to create a catalog of cloneable tools.
    // When use_allowed is true, tools are sandboxed to allowed directories.
    // When false, tools can access any path.
    let tools = default_tools(true, resolver.clone(), TodoState::new());

    // === Build registry ===
    //
    // AgentDefaults specifies the default model and sampling parameters
    // for agents that don't override them in their config.
    let defaults = AgentDefaults {
        model: OPENAI_MODEL.to_string(),
        api_key: Some(get_openai_api_key()),
        base_url: Some(OPENAI_BASE_URL.to_string()),
        temperature: None,
        top_p: None,
        options: Default::default(),
    };

    // Build the registry from the catalog and tool catalog.
    // The registry prebuilds all agents with their allowed tools from the catalog.
    //
    // Note: For OpenAI models with "openai:" prefix, AgentBuilder::from_model
    // will resolve the model using environment variables like OPENAI_API_KEY.
    let registry = AgentRegistryBuilder::<()>::new(defaults, tools).build(&catalog)?;

    // === Task tool permissions (allow Task for the single subagent only) ===
    //
    // The caller_rules control which subagents the primary agent can invoke.
    // Here we only allow the one "file-reader" subagent.
    let mut caller_rules = Ruleset::new();
    caller_rules.push(Rule::new("task", "file-reader", PermissionAction::Allow));
    let deps = Arc::new(());
    let task_tool = TaskTool::new(Arc::new(registry), caller_rules, deps);

    // === Build primary agent with Task tool only ===
    //
    // Build a system prompt that includes working directory and optionally allowed paths.
    let mut pb = SystemPromptBuilder::new()
        .working_directory(std::env::current_dir()?.display().to_string());
    if let Some(ref resolver) = resolver {
        pb = pb.allowed_paths(resolver);
    }

    // Create the primary agent with ONLY the Task tool (forces delegation to subagent).
    //
    // Note: For OpenAI models with "openai:" prefix, use ModelConfig to set custom base URL.
    let agent = AgentBuilder::<(), String>::from_config(
        ModelConfig::new(OPENAI_MODEL)
            .with_api_key(get_openai_api_key())
            .with_base_url(OPENAI_BASE_URL),
    )?
    .tool(pb.track(task_tool))
    .system_prompt(pb.build())
    .build();

    // === Print tool info ===
    println!("=== Agent Ready ({} tools) ===", agent.tools().len());

    // === Invoke a subagent via Task ===
    //
    // Prompt the primary agent to use the Task tool to invoke a subagent.
    // The primary agent must delegate because it only has the Task tool.
    let prompt = "Use the Task tool with subagent_type 'file-reader' to read Cargo.toml and summarize dependencies.";
    println!("\n=== Running Agent ===");

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
