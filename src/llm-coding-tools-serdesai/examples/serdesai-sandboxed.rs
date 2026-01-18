//! Sandboxed tools example - restricted file access.
//!
//! Demonstrates using `allowed` tools that restrict file operations
//! to specific directories only. This is useful for:
//!
//! - Multi-tenant environments where agents should only access their workspace
//! - Security-conscious deployments limiting filesystem exposure
//! - Project-scoped agents that shouldn't touch system files
//!
//! Run: cargo run --example sandboxed -p llm-coding-tools-serdesai

use llm_coding_tools_serdesai::PreambleBuilder;
use llm_coding_tools_serdesai::allowed::{EditTool, GlobTool, GrepTool, ReadTool, WriteTool};
use serdes_ai::tools::ToolRegistry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // === Define allowed directories ===
    //
    // Only these directories (and their subdirectories) will be accessible.
    // Attempts to read/write outside these paths will fail with an error.
    let allowed_paths = vec![
        std::env::current_dir()?, // Current working directory
        std::env::temp_dir(),     // Temp directory (cross-platform)
    ];

    // === Create tools with allowed paths ===
    //
    // Each tool is initialized with the same set of allowed directories.
    // The `allowed` module tools use `AllowedPathResolver` internally.
    let read: ReadTool<true> = ReadTool::new(allowed_paths.clone())?;
    let write = WriteTool::new(allowed_paths.clone())?;
    let edit = EditTool::new(allowed_paths.clone())?;
    let glob = GlobTool::new(allowed_paths.clone())?;
    let grep: GrepTool<true> = GrepTool::new(allowed_paths)?;

    // === Build registry with preamble tracking ===
    let mut pb = PreambleBuilder::<false>::new();
    let mut registry = ToolRegistry::<()>::new();

    registry.register(pb.track(read));
    registry.register(pb.track(write));
    registry.register(pb.track(edit));
    registry.register(pb.track(glob));
    registry.register(pb.track(grep));

    let preamble = pb.build();

    // Print the preamble
    println!("{preamble}");

    Ok(())
}
