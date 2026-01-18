//! Basic tools example - demonstrates tool setup with serdesAI.
//!
//! Shows:
//! - Creating tools individually
//! - Using [`PreambleBuilder`] for context generation
//! - Using [`AgentBuilderExt`] to add tools to an agent
//! - Running the agent with tools
//!
//! Run: OPENAI_API_KEY=... cargo run --example serdesai-basic -p llm-coding-tools-serdesai

use llm_coding_tools_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use llm_coding_tools_serdesai::{BashTool, PreambleBuilder, WebFetchTool, create_todo_tools};
use serdes_ai::prelude::*;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    // === Create preamble builder to track tools ===
    let mut pb = PreambleBuilder::<false>::new();

    // === Create todo tools with shared state ===
    let (todo_read, todo_write, _state) = create_todo_tools();

    // === Build agent with tools - call .system_prompt() last ===
    let agent = AgentBuilder::<(), String>::from_model("openai:gpt-4o")?
        // File operations
        .tool(pb.track(ReadTool::<true>::new()))
        .tool(pb.track(GlobTool::new()))
        .tool(pb.track(GrepTool::<true>::new()))
        // Shell execution
        .tool(pb.track(BashTool::new()))
        // Web content fetching
        .tool(pb.track(WebFetchTool::new()))
        // Todo tools with shared state
        .tool(pb.track(todo_read))
        .tool(pb.track(todo_write))
        // System prompt last (after tracking all tools)
        .system_prompt(pb.build())
        .build();

    // === Print tool info ===
    println!("=== Agent Ready ({} tools) ===", agent.tools().len());

    // === Run the agent ===
    println!("\n=== Running Agent ===");
    let result = agent
        .run(
            "List the Rust files in the current directory using glob",
            (),
        )
        .await?;
    println!("\n=== Response ===\n{}", result.output());

    Ok(())
}
