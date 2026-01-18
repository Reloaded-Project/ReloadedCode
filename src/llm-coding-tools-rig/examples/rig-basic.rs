//! PreambleBuilder example - pass-through tracking for ToolSet.
//!
//! Demonstrates:
//! - Using PreambleBuilder alongside ToolSet::builder()
//! - Full access to Rig's API (no wrapper limitations)
//! - TodoTools with shared state
//! - Generating and using the preamble string
//!
//! Run: cargo run --example rig-basic -p llm-coding-tools-rig

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
    let _toolset = ToolSet::builder()
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

    // Print the preamble
    println!("{preamble}");
}
