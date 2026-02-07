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
use llm_coding_tools_agents::{AgentCatalog, AgentLoader};
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, AllowedPathResolver, ModelsDevResolver, ProviderOverride,
    ProviderOverrides, TodoState, default_tools,
};
use serdes_ai::prelude::*;
use std::fmt::Write;
use std::sync::Arc;

// Set your OpenAI API key here or via OPENAI_API_KEY environment variable.
const OPENAI_API_KEY: &str = "";
const OPENAI_MODEL: &str = "openai:hf:zai-org/GLM-4.7";
const OPENAI_BASE_URL: &str = "https://api.synthetic.new/openai/v1";

fn get_openai_api_key() -> String {
    std::env::var("OPENAI_API_KEY")
        .or_else(|_| {
            if !OPENAI_API_KEY.is_empty() {
                Ok(OPENAI_API_KEY.to_string())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .expect(
            "OPENAI_API_KEY not set: please provide OPENAI_API_KEY env var or set OPENAI_API_KEY constant",
        )
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
    let allowed_path_resolver = if use_allowed {
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
    let tools = default_tools(true, allowed_path_resolver.clone(), TodoState::new());

    // === Load models.dev catalog and build model resolver ===
    //
    let models_dev_catalog = ModelsDevCatalog::load_shared_cache_or_bundled()?.catalog;
    let provider_overrides = ProviderOverrides::new().insert_override(
        "openai",
        ProviderOverride {
            api_key: Some(get_openai_api_key()),
            base_url: Some(OPENAI_BASE_URL.to_string()),
            endpoint_env: None,
        },
    );
    let model_resolver =
        ModelsDevResolver::new(Some(models_dev_catalog), provider_overrides.clone());

    // === Build registry ===
    //
    // AgentDefaults specifies the default model and sampling parameters
    // for agents that don't override them in their config.
    let defaults = AgentDefaults {
        model: OPENAI_MODEL.to_string(),
        model_resolver: Some(model_resolver.clone()),
        provider_overrides,
        api_key: None,
        base_url: None,
        temperature: None,
        top_p: None,
        options: Default::default(),
    };

    // Build the registry with recursive Task wiring enabled.
    //
    // The registry prebuilds all agents with their allowed tools from the catalog.
    // Recursive Task availability is controlled by each agent's permission.task rules.
    // Agents with allow rules for task can delegate to other agents.
    let deps = Arc::new(());
    let registry = AgentRegistryBuilder::<()>::new(defaults, tools)
        .build_with_recursive_task(&catalog, Arc::clone(&deps))?;

    // Primary agent comes from the same catalog and already carries Task
    // wiring according to its own permission.task rules.
    // For this example, we use the file-reader agent as the entry point.
    let primary = registry
        .get("file-reader")
        .ok_or_else(|| "missing file-reader agent".to_string())?;

    // === Print tool info ===
    println!(
        "=== Agent Ready ({} tools) ===",
        primary.agent.tools().len()
    );

    // === Invoke a subagent via Task ===
    //
    // Prompt the primary agent to use the Task tool to invoke a subagent.
    // The primary agent must delegate because it only has the Task tool.
    let prompt = "Use the Task tool with subagent_type 'file-reader' to read Cargo.toml and summarize dependencies.";
    println!("\n=== Running Agent ===");

    let mut stream = primary.agent.run_stream(prompt, Arc::clone(&deps)).await?;

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
