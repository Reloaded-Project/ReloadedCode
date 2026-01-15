//! PreambleBuilder example - pass-through tracking for ToolSet.
//!
//! Demonstrates:
//! - Using PreambleBuilder alongside ToolSet::builder()
//! - Full access to Rig's API (no wrapper limitations)
//! - TodoTools with shared state
//! - Generating and using the preamble string
//!
//! Run: cargo run --example basic -p llm-coding-tools-rig
//!
//! For a complete agent setup, see: cargo run --example full_agent -p llm-coding-tools-rig

use llm_coding_tools_rig::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_rig::{BashTool, PreambleBuilder, TodoTools};
use rig::tool::ToolSet;

#[tokio::main]
async fn main() {
    // === Create shared state for todos ===
    let todos = TodoTools::new();

    // === Create preamble builder to track tools ===
    let mut pb = PreambleBuilder::<false>::new();

    // === Use ToolSet::builder() directly - full Rig API! ===
    let toolset = ToolSet::builder()
        .static_tool(pb.track(ReadTool::<true>::new()))
        .static_tool(pb.track(GlobTool::new()))
        .static_tool(pb.track(GrepTool::<true>::new()))
        .static_tool(pb.track(BashTool::new()))
        // Todo tools share state for read/write coordination
        .static_tool(pb.track(todos.read))
        .static_tool(pb.track(todos.write))
        // Can use any ToolSet method here - dynamic_tool, etc.
        .build();

    // === Generate preamble string ===
    let preamble = pb.build();

    // === Print tool definitions from ToolSet ===
    println!("=== Tools in ToolSet ===");
    for def in toolset.get_tool_definitions().await.unwrap() {
        let truncated_desc: String = def.description.chars().take(60).collect();
        println!("  - {}: {}", def.name, truncated_desc);
    }

    // === Print generated preamble ===
    println!("\n=== Generated Preamble ({} chars) ===\n", preamble.len());
    let truncated_preamble: String = preamble.chars().take(1000).collect();
    println!("{}", truncated_preamble);
    if preamble.len() > 1000 {
        println!("\n... ({} more chars)", preamble.len() - 1000);
    }

    // === Integration with Rig agent ===
    // IMPORTANT: You must call .preamble() to actually use the generated string!
    //
    // let agent = openai::Client::from_env()
    //     .agent("gpt-4o")
    //     .preamble(&preamble)  // <-- Pass preamble to Rig
    //     .tools(toolset)
    //     .build();
    //
    // let response = agent.prompt("Read main.rs").await?;
}
