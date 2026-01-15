//! Complete agent example - demonstrates full integration pattern.
//!
//! This example shows the recommended way to build an LLM coding agent
//! with all available tools. Agent execution is commented out as it
//! requires API credentials.
//!
//! Run: cargo run --example full_agent -p llm-coding-tools-rig

use llm_coding_tools_rig::absolute::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};
use llm_coding_tools_rig::{BashTool, PreambleBuilder, TodoTools, WebFetchTool};
use rig::tool::ToolSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // === 1. Create shared state for todos ===
    //
    // TodoTools provides paired read/write tools that share state.
    // This allows the LLM to maintain a task list across the conversation.
    let todos = TodoTools::new();

    // === 2. Create preamble builder ===
    //
    // PreambleBuilder tracks which tools are registered and generates
    // a combined context string for the system prompt. This gives the
    // LLM detailed guidance on how to use each tool effectively.
    let mut pb = PreambleBuilder::<false>::new();

    // === 3. Build toolset with all tools ===
    //
    // Use pb.track() to wrap each tool - this registers it with the
    // preamble builder while passing it through unchanged to the toolset.
    let toolset = ToolSet::builder()
        // File operations (with line numbers enabled)
        .static_tool(pb.track(ReadTool::<true>::new()))
        .static_tool(pb.track(WriteTool::new()))
        .static_tool(pb.track(EditTool::new()))
        .static_tool(pb.track(GlobTool::new()))
        .static_tool(pb.track(GrepTool::<true>::new()))
        // Shell execution
        .static_tool(pb.track(BashTool::new()))
        // Web content fetching
        .static_tool(pb.track(WebFetchTool::new()))
        // Todo management (shared state between read and write)
        .static_tool(pb.track(todos.read))
        .static_tool(pb.track(todos.write))
        .build();

    // === 4. Generate preamble ===
    //
    // The preamble contains usage instructions for all tracked tools.
    // Pass this to the agent's .preamble() method so the LLM knows
    // how to use the tools correctly.
    let preamble = pb.build();

    // === 5. Agent integration (requires API key) ===
    //
    // Uncomment and configure with your preferred LLM provider:
    //
    // ```
    // use rig::providers::openai;
    //
    // let client = openai::Client::from_env();
    // let agent = client
    //     .agent("gpt-4o")
    //     .preamble(&preamble)
    //     .tools(toolset)
    //     .build();
    //
    // // Example prompts this agent can handle:
    // let response = agent.prompt("Find all Rust files in src/").await?;
    // let response = agent.prompt("Read Cargo.toml and summarize dependencies").await?;
    // let response = agent.prompt("Search for TODO comments in the codebase").await?;
    // let response = agent.prompt("Run 'cargo test' and report results").await?;
    // let response = agent.prompt("Fetch https://example.com and summarize").await?;
    // ```

    // === Demo output ===
    let tool_count = toolset.get_tool_definitions().await?.len();

    println!("=== Full Agent Configuration ===\n");
    println!("Tools registered: {}", tool_count);
    println!("Preamble size: {} chars\n", preamble.len());

    println!("=== Registered Tools ===");
    for def in toolset.get_tool_definitions().await? {
        // Show first 60 chars of description
        let desc = &def.description[..60.min(def.description.len())];
        println!("  {}: {}...", def.name, desc);
    }

    println!("\n=== Example Prompts ===");
    println!("  - \"Find all Rust files in src/\"");
    println!("  - \"Read Cargo.toml and list dependencies\"");
    println!("  - \"Search for TODO comments\"");
    println!("  - \"Run 'cargo test' and report results\"");
    println!("  - \"Create a todo list for implementing feature X\"");

    println!("\n=== Preamble Preview (first 500 chars) ===\n");
    println!("{}", &preamble[..500.min(preamble.len())]);
    if preamble.len() > 500 {
        println!("\n... ({} more chars)", preamble.len() - 500);
    }

    Ok(())
}
