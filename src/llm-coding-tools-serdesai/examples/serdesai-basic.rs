//! Basic tools example - demonstrates tool setup with serdesAI.
//!
//! Shows:
//! - Creating tools individually
//! - Using [`PreambleBuilder`] for context generation
//! - Registering tools with [`ToolRegistry`]
//!
//! Run: cargo run --example basic -p llm-coding-tools-serdesai

use llm_coding_tools_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_serdesai::{BashTool, PreambleBuilder, WebFetchTool, create_todo_tools};
use serdes_ai::tools::ToolRegistry;

#[tokio::main]
async fn main() {
    // === Create preamble builder to track tools ===
    let mut pb = PreambleBuilder::<false>::new();

    // === Create and register tools with ToolRegistry ===
    let mut registry = ToolRegistry::<()>::new();

    // File operations
    registry.register(pb.track(ReadTool::<true>::new()));
    registry.register(pb.track(GlobTool::new()));
    registry.register(pb.track(GrepTool::<true>::new()));

    // Shell execution
    registry.register(pb.track(BashTool::new()));

    // Web content fetching
    registry.register(pb.track(WebFetchTool::new()));

    // Todo tools with shared state
    let (todo_read, todo_write, _state) = create_todo_tools();
    registry.register(pb.track(todo_read));
    registry.register(pb.track(todo_write));

    // === Generate preamble string ===
    let preamble = pb.build();

    // === Print tool definitions from registry ===
    println!("=== Tools in Registry ({}) ===", registry.len());
    for def in registry.definitions() {
        println!("  - {}: {}", def.name, def.description);
    }

    // === Print generated preamble ===
    println!(
        "\n=== Generated Preamble ({} chars) ===\n",
        preamble.chars().count()
    );
    println!("{}", preamble);

    // === Integration with serdesAI Agent ===
    // IMPORTANT: Pass the preamble to your agent's system prompt!
    //
    // let agent = Agent::builder()
    //     .model("openai:gpt-4o")
    //     .system_prompt(&preamble)
    //     .tools(registry)
    //     .build()?;
    //
    // let response = agent.run("Read Cargo.toml", ()).await?;
}
