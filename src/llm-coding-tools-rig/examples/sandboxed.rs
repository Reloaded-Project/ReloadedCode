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
    let allowed_paths = vec![
        std::env::current_dir()?, // Current working directory
        PathBuf::from("/tmp"),    // Temp directory
    ];

    // === Create resolver and tools ===
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
    let _toolset = ToolSet::builder()
        .static_tool(pb.track(read))
        .static_tool(pb.track(write))
        .static_tool(pb.track(edit))
        .static_tool(pb.track(glob))
        .static_tool(pb.track(grep))
        .build();

    let preamble = pb.build();

    // Print the preamble
    println!("{preamble}");

    Ok(())
}
