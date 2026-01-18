//! Sandboxed tools example - restricted file access.
//!
//! Demonstrates using `allowed` tools that restrict file operations
//! to specific directories only. This is useful for:
//!
//! - Multi-tenant environments where agents should only access their workspace
//! - Security-conscious deployments limiting filesystem exposure
//! - Project-scoped agents that shouldn't touch system files
//!
//! Run: OPENAI_API_KEY=... cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

use llm_coding_tools_serdesai::PreambleBuilder;
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use llm_coding_tools_serdesai::allowed::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};
use serdes_ai::prelude::*;

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

    // === Create tools with allowed paths ===
    let read: ReadTool<true> = ReadTool::new(allowed_paths.clone()).unwrap();
    let write = WriteTool::new(allowed_paths.clone()).unwrap();
    let edit = EditTool::new(allowed_paths.clone()).unwrap();
    let glob = GlobTool::new(allowed_paths.clone()).unwrap();
    let grep: GrepTool<true> = GrepTool::new(allowed_paths).unwrap();

    // === Build agent with sandboxed tools - call .system_prompt() last ===
    let mut pb = PreambleBuilder::<false>::new();
    let agent = AgentBuilder::<(), String>::from_model("openai:gpt-4o")?
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
    let result = agent
        .run("List all Rust source files in the current directory", ())
        .await?;
    println!("\n=== Response ===\n{}", result.output());

    Ok(())
}
