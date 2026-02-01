//! Registry-driven Task tool example (rig).
//!
//! Demonstrates:
//! - Loading agent configs from directory or fallback to inline config
//! - Using default_tools to build the tool catalog
//! - Building an AgentRegistry from AgentCatalog and tools
//! - Creating a TaskTool for subagent invocation
//! - Setting up a primary agent with Task tool
//! - Running a simple task that invokes a subagent
//!
//! Run: cargo run --example registry-driven-task-rig -p llm-coding-tools-rig

use llm_coding_tools_agents::{AgentCatalog, AgentLoader, PermissionAction, Rule, Ruleset};
use llm_coding_tools_rig::{
    AgentDefaults, AgentRegistryBuilder, AllowedPathResolver, SystemPromptBuilder, TaskTool,
    TodoState, default_tools,
};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::openrouter;
use std::sync::Arc;

// Set your OpenRouter API key here or via OPENROUTER_API_KEY environment variable.
// Using a free model, so minimal/no charges expected.
const OPENROUTER_API_KEY: &str = "";
const OPENROUTER_MODEL: &str = "z-ai/glm-4.5-air:free";

// Read API key from environment with fallback to default constant
fn get_openrouter_api_key() -> String {
    std::env::var("OPENROUTER_API_KEY")
        .unwrap_or_else(|_| OPENROUTER_API_KEY.to_string())
}

// Fallback agent config used when config directory is empty or missing.
const DEFAULT_AGENT: &str = "---\nmode: subagent\ndescription: Example subagent\npermission:\n  read: allow\n  glob: allow\n---\nYou are a helpful subagent. Respond concisely.\n";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // === Load agent configs ===
    //
    // Load configs from OPENCODE_AGENT_DIR environment variable or use ".opencode".
    // If no configs are found, use the inline DEFAULT_AGENT fallback.
    let config_dir = std::env::var("OPENCODE_AGENT_DIR").unwrap_or_else(|_| ".opencode".into());
    let loader = AgentLoader::new();
    let mut catalog = AgentCatalog::new();
    loader.add_directory(&mut catalog, &config_dir)?;
    if catalog.iter().next().is_none() {
        // Add a fallback agent so the example works without external config files
        loader.add_from_str(&mut catalog, DEFAULT_AGENT, "example-subagent")?;
    }

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
        model: OPENROUTER_MODEL.to_string(),
        temperature: None,
        top_p: None,
        options: Default::default(),
    };

    // Create the rig client and build the registry from the catalog.
    // The registry prebuilds all agents with their allowed tools from the catalog.
    let client: openrouter::Client = openrouter::Client::new(&get_openrouter_api_key())?;
    let registry = AgentRegistryBuilder::new(|model| client.agent(model), defaults, tools)
        .build(&catalog)?;

    // === Task tool permissions (allow Task for all subagents) ===
    //
    // The caller_rules control which subagents the primary agent can invoke.
    // Here we allow invocation of all agent types ("*").
    let mut caller_rules = Ruleset::new();
    caller_rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let task_tool = TaskTool::new(Arc::new(registry), caller_rules);

    // === Build primary agent with Task tool ===
    //
    // Build a system prompt that includes working directory and optionally allowed paths.
    let mut pb = SystemPromptBuilder::new()
        .working_directory(std::env::current_dir()?.display().to_string());
    if let Some(ref resolver) = resolver {
        pb = pb.allowed_paths(resolver);
    }

    // Create the primary agent and register the Task tool.
    let agent = client
        .agent(OPENROUTER_MODEL)
        .tool(task_tool)
        .preamble(&pb.build())
        .build();

    // === Invoke a subagent via Task ===
    //
    // Prompt the primary agent to use the Task tool to invoke a subagent.
    // The subagent_type "example-subagent" matches the fallback config above.
    let prompt = "Use the Task tool with subagent_type 'example-subagent' to say hello.";
    println!("Prompt: {}\n", prompt);
    println!("Response:");
    let response = agent.prompt(prompt).await?;
    println!("{response}");

    Ok(())
}
