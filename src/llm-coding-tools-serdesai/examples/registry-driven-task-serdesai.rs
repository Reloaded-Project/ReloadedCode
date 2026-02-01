//! Registry-driven Task tool example (serdesAI).
//!
//! Demonstrates:
//! - Loading agent configs from directory or fallback to inline config
//! - Using default_tools to build the tool catalog
//! - Building an AgentRegistry from AgentCatalog and tools
//! - Creating a TaskTool for subagent invocation
//! - Setting up a primary agent with Task tool
//! - Running a simple task that invokes a subagent
//!
//! Run: cargo run --example registry-driven-task-serdesai -p llm-coding-tools-serdesai

use llm_coding_tools_agents::{AgentCatalog, AgentLoader, PermissionAction, Rule, Ruleset};
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, AllowedPathResolver, SystemPromptBuilder, TaskTool,
    TodoState, default_tools,
};
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use serdes_ai::prelude::*;
use std::sync::Arc;

// For OpenRouter, set OPENROUTER_API_KEY in the environment.
// The model string uses the "openrouter:" prefix which is resolved by serdesAI.
const OPENROUTER_MODEL: &str = "openrouter:z-ai/glm-4.5-air:free";

// Fallback agent config used when config directory is empty or missing.
const DEFAULT_AGENT: &str = "---\nmode: subagent\ndescription: Example subagent\npermission:\n  read: allow\n  glob: allow\n---\nYou are a helpful subagent. Respond concisely.\n";

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
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

    // Build the registry from the catalog and tool catalog.
    // The registry prebuilds all agents with their allowed tools from the catalog.
    //
    // Note: AgentBuilder::from_model depends on provider config being available.
    // For OpenRouter, ensure OPENROUTER_API_KEY environment variable is set.
    let registry = AgentRegistryBuilder::<()>::new(defaults, tools).build(&catalog)?;

    // === Task tool permissions (allow Task for all subagents) ===
    //
    // The caller_rules control which subagents the primary agent can invoke.
    // Here we allow invocation of all agent types ("*").
    let mut caller_rules = Ruleset::new();
    caller_rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let deps = Arc::new(());
    let task_tool = TaskTool::new(Arc::new(registry), caller_rules, deps);

    // === Build primary agent with Task tool ===
    //
    // Build a system prompt that includes working directory and optionally allowed paths.
    let mut pb = SystemPromptBuilder::new()
        .working_directory(std::env::current_dir()?.display().to_string());
    if let Some(ref resolver) = resolver {
        pb = pb.allowed_paths(resolver);
    }

    // Create the primary agent using AgentBuilderExt to register the Task tool.
    //
    // Note: For OpenRouter models with "openrouter:" prefix, AgentBuilder::from_model
    // will resolve the model using environment variables like OPENROUTER_API_KEY.
    let agent = AgentBuilder::<(), String>::from_model(OPENROUTER_MODEL)?
        .tool(pb.track(task_tool))
        .system_prompt(pb.build())
        .build();

    // === Invoke a subagent via Task ===
    //
    // Prompt the primary agent to use the Task tool to invoke a subagent.
    // The subagent_type "example-subagent" matches the fallback config above.
    let prompt = "Use the Task tool with subagent_type 'example-subagent' to say hello.";
    println!("Prompt: {}\n", prompt);
    println!("Response:");

    let response = agent.run(prompt, ()).await?;
    println!("{}", response.output());

    Ok(())
}
