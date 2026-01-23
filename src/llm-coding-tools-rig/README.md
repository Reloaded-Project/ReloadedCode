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

```rust,no_run
use llm_coding_tools_rig::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_rig::{BashTool, SystemPromptBuilder, TodoTools};
use rig::providers::openai;
use rig::client::{ProviderClient, CompletionClient};
use rig::completion::Prompt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let todos = TodoTools::new();
    let mut pb = SystemPromptBuilder::new();

    // Build agent with system prompt tracking
    let client = openai::Client::from_env();
    let agent = client
        .agent("gpt-4o")
        .tool(pb.track(ReadTool::<true>::new()))
        .tool(pb.track(GlobTool::new()))
        .tool(pb.track(GrepTool::<true>::new()))
        .tool(pb.track(BashTool::new()))
        .tool(pb.track(todos.read))
        .tool(pb.track(todos.write))
        .preamble(&pb.build())  // Build system prompt after tracking tools
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
# Environment

Working directory: /home/user/project

# Tool Usage Guidelines

## `Read` Tool
Reads files from disk.
## `Bash` Tool
Executes shell commands.
```

## Usage

File tools come in `absolute::*` (unrestricted) and `allowed::*` (sandboxed) variants:

```rust,no_run
use llm_coding_tools_rig::absolute::{ReadTool, WriteTool};
use llm_coding_tools_rig::allowed::{ReadTool as AllowedReadTool, WriteTool as AllowedWriteTool};
use llm_coding_tools_rig::AllowedPathResolver;
use std::path::PathBuf;

let read = ReadTool::<true>::new();
let resolver = AllowedPathResolver::new([PathBuf::from("/home/user/project")]).unwrap();
let sandboxed_read: AllowedReadTool<true> = AllowedReadTool::new(resolver.clone());
let sandboxed_write = AllowedWriteTool::new(resolver);
```

Other tools: `BashTool`, `TaskTool`, `WebFetchTool`, `TodoTools`.
Use `SystemPromptBuilder` to register tools and pass `pb.build()` to `.preamble()`. Set `working_directory()` so that environment section is populated.
Context strings are re-exported in `llm_coding_tools_rig::context` (e.g., `BASH`, `READ_ABSOLUTE`).

## Examples

```bash
# Basic toolset setup with SystemPromptBuilder
cargo run --example basic -p llm-coding-tools-rig

# Sandboxed file access with allowed::* tools
cargo run --example sandboxed -p llm-coding-tools-rig
```

## License

Apache 2.0
