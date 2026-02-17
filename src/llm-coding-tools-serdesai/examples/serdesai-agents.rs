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
//! Run: SYNTHETIC_API_KEY=... cargo run --example serdesai-agents -p llm-coding-tools-serdesai
//! Or set SYNTHETIC_API_KEY in the const below.

use futures::StreamExt;
use llm_coding_tools_agents::{AgentCatalog, AgentLoader};
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, AllowedPathResolver, ProviderOverride, ProviderOverrides,
    TodoState, default_tools,
};
use serdes_ai::prelude::*;
use std::fmt::Write;
use std::sync::Arc;

// Model and provider are inherited from MODEL_SPEC (parsed from models.dev format).
const MODEL_SPEC: &str = "synthetic/hf:zai-org/GLM-4.7";

// Set your Synthetic API key here or via SYNTHETIC_API_KEY environment variable.
/// Fallback API key if env var is not set. Leave empty to require env var.
const SYNTHETIC_API_KEY: &str = "";

fn get_synthetic_api_key() -> String {
    std::env::var("SYNTHETIC_API_KEY").unwrap_or_else(|_| {
        if SYNTHETIC_API_KEY.is_empty() {
            panic!("SYNTHETIC_API_KEY environment variable must be set");
        }
        SYNTHETIC_API_KEY.to_string()
    })
}

fn provider_overrides_from_env() -> ProviderOverrides {
    let endpoint_override = std::env::var("SYNTHETIC_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let api_key = get_synthetic_api_key();

    ProviderOverrides::new().insert_override(
        "synthetic",
        ProviderOverride {
            api_key: Some(api_key),
            base_url: endpoint_override,
            endpoint_env: None,
        },
    )
}

// Embedded subagent config (loaded via include_str!)
const SUBAGENT_CONFIG: &str = include_str!("agents/file-reader.md");

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // === Load agent config ===
    //
    // Load a single embedded agent config using include_str!.
    let loader = AgentLoader::new();
    let mut catalog = AgentCatalog::new();
    loader.add_from_str(&mut catalog, SUBAGENT_CONFIG, "file-reader")?;

    // === Setup allowed paths for sandboxed tools ===
    //
    // Tools are sandboxed to the current directory and temp directory.
    let allowed_path_resolver =
        AllowedPathResolver::new([std::env::current_dir()?, std::env::temp_dir()])?;

    // === Build tool catalog ===
    //
    // Use default_tools to create a catalog of cloneable tools.
    // Tools are sandboxed to allowed directories.
    let tools = default_tools(true, Some(allowed_path_resolver), TodoState::new());

    let provider_overrides = provider_overrides_from_env();

    // === Build registry ===
    //
    // AgentDefaults specifies model resolution + sampling parameters.
    // model_resolver: None uses the default resolver abstraction (models.dev-backed).
    // api_key is set via ProviderOverrides above (required for non-OpenAI providers).
    let defaults = AgentDefaults {
        model: MODEL_SPEC.to_string(),
        model_resolver: None,
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
