//! Sandboxed tools example - restricted file access.
//!
//! Demonstrates using `allowed::*` tools that restrict file operations
//! to specific directories only. This is useful for:
//!
//! - Multi-tenant environments where agents should only access their workspace
//! - Security-conscious deployments limiting filesystem exposure
//! - Project-scoped agents that shouldn't touch system files
//!
//! Run: cargo run --example sandboxed -p llm-coding-tools-rig

use llm_coding_tools_rig::allowed::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};
use llm_coding_tools_rig::{AllowedPathResolver, PreambleBuilder};
use rig::tool::ToolSet;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // === Define allowed directories ===
    //
    // Only these directories (and their subdirectories) will be accessible.
    // Attempts to read/write outside these paths will fail with an error.
    //
    // NOTE: Paths must exist - AllowedPathResolver canonicalizes them.
    // Using current directory and /tmp as they exist on most systems.
    let current_dir = std::env::current_dir()?;
    let allowed_paths = vec![
        current_dir.clone(),   // Current working directory
        PathBuf::from("/tmp"), // Temp directory
    ];

    println!("=== Sandboxed Agent Configuration ===\n");
    println!("Allowed directories:");
    for path in &allowed_paths {
        println!("  - {}", path.display());
    }

    // === Option 1: Create tools individually ===
    //
    // Each tool gets its own copy of the allowed paths.
    // Simple but duplicates the path list.
    let _read: ReadTool<true> = ReadTool::new(allowed_paths.clone())?;
    let _write = WriteTool::new(allowed_paths.clone())?;

    // === Option 2: Share a resolver (recommended) ===
    //
    // Create one resolver and share it across tools.
    // More efficient and ensures consistency.
    let resolver = AllowedPathResolver::new(allowed_paths)?;

    let read: ReadTool<true> = ReadTool::with_resolver(resolver.clone());
    let write = WriteTool::with_resolver(resolver.clone());
    let edit = EditTool::with_resolver(resolver.clone());
    let glob = GlobTool::with_resolver(resolver.clone());
    let grep: GrepTool<true> = GrepTool::with_resolver(resolver);

    // === Build toolset ===
    let mut pb = PreambleBuilder::<false>::new();
    let toolset = ToolSet::builder()
        .static_tool(pb.track(read))
        .static_tool(pb.track(write))
        .static_tool(pb.track(edit))
        .static_tool(pb.track(glob))
        .static_tool(pb.track(grep))
        .build();

    let preamble = pb.build();

    // === Demo output ===
    println!(
        "\nTools registered: {}",
        toolset.get_tool_definitions().await?.len()
    );
    println!("Preamble size: {} chars", preamble.len());

    println!("\n=== Security Behavior ===");
    println!("  Allowed:  read(\"{}/Cargo.toml\")", current_dir.display());
    println!("  Allowed:  glob(\"/tmp/**/*.txt\")");
    println!("  BLOCKED:  read(\"/etc/passwd\")");
    println!("  BLOCKED:  write(\"/home/user/.ssh/config\")");

    println!("\n=== Error Handling ===");
    println!("  When a path is outside allowed directories, tools return:");
    println!("  ToolError::InvalidPath(\"path not within allowed directories\")");

    println!("\n=== Agent Integration ===");
    println!("  The preamble automatically includes 'allowed path' context,");
    println!("  informing the LLM that paths are relative to allowed directories.");

    Ok(())
}
