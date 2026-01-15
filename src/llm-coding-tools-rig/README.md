# llm-coding-tools-rig

[![Crates.io](https://img.shields.io/crates/v/llm-coding-tools-rig.svg)](https://crates.io/crates/llm-coding-tools-rig)
[![Docs.rs](https://docs.rs/llm-coding-tools-rig/badge.svg)](https://docs.rs/llm-coding-tools-rig)

Lightweight, high-performance Rig framework Tool implementations for coding tools.

## Features

- **File operations** - Read, write, edit, glob, grep with two access modes:
  - `absolute::*` - Unrestricted filesystem access
  - `allowed::*` - Sandboxed to configured directories
- **Shell execution** - Cross-platform command execution with timeout
- **Web fetching** - URL content retrieval with format conversion
- **Task delegation** - Sub-agent spawning for complex tasks
- **Todo management** - Shared-state todo list tracking
- **Context strings** - LLM guidance text for tool usage (re-exported from core)

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
llm-coding-tools-rig = "0.1"
```

## Quick Start

Minimal runnable agent (requires `OPENAI_API_KEY`):

```rust
use llm_coding_tools_rig::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_rig::{BashTool, PreambleBuilder, TodoTools};
use rig::providers::openai;
use rig::tool::ToolSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let todos = TodoTools::new();
    let mut pb = PreambleBuilder::<false>::new();

    let toolset = ToolSet::builder()
        .static_tool(pb.track(ReadTool::<true>::new()))
        .static_tool(pb.track(GlobTool::new()))
        .static_tool(pb.track(GrepTool::<true>::new()))
        .static_tool(pb.track(BashTool::new()))
        .static_tool(pb.track(todos.read))
        .static_tool(pb.track(todos.write))
        .build();

    let preamble = pb.build();

    let client = openai::Client::from_env();
    let agent = client
        .agent("gpt-4o")
        .preamble(&preamble)
        .tools(toolset)
        .build();

    let response = agent
        .prompt("Search for TODO comments in src/")
        .await?;
    println!("{response}");

    Ok(())
}
```

Example preamble output (truncated):

```text
# Tool Usage Guidelines

## Read Tool

Reads files from disk.

## Bash Tool

Executes shell commands.
```

Run the full example app:

```bash
OPENAI_API_KEY=... cargo run --example full_agent -p llm-coding-tools-rig
```

## Usage

File tools come in `absolute::*` (unrestricted) and `allowed::*` (sandboxed) variants:

```rust
use llm_coding_tools_rig::absolute::{ReadTool, WriteTool};
use llm_coding_tools_rig::allowed::{ReadTool as AllowedReadTool, WriteTool as AllowedWriteTool};
use llm_coding_tools_rig::AllowedPathResolver;
use std::path::PathBuf;

let read = ReadTool::<true>::new();
let resolver = AllowedPathResolver::new([PathBuf::from("/home/user/project")]).unwrap();
let sandboxed_read: AllowedReadTool<true> = AllowedReadTool::with_resolver(resolver.clone());
let sandboxed_write = AllowedWriteTool::with_resolver(resolver);
```

Other tools: `BashTool`, `WebFetchTool`, `TaskTool`, `TodoTools`.
Use `PreambleBuilder` to register tools and pass `pb.build()` to `.preamble()`.
Context strings are re-exported in `llm_coding_tools_rig::context` (e.g., `BASH`, `READ_ABSOLUTE`).

## Examples

```bash
# Basic toolset setup with PreambleBuilder
cargo run --example basic -p llm-coding-tools-rig

# Complete agent configuration (recommended starting point)
cargo run --example full_agent -p llm-coding-tools-rig

# Sandboxed file access with allowed::* tools
cargo run --example sandboxed -p llm-coding-tools-rig
```

## License

Apache 2.0
