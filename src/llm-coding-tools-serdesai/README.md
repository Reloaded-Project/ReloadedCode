# llm-coding-tools-serdesai

[![Crates.io](https://img.shields.io/crates/v/llm-coding-tools-serdesai.svg)](https://crates.io/crates/llm-coding-tools-serdesai)
[![Docs.rs](https://docs.rs/llm-coding-tools-serdesai/badge.svg)](https://docs.rs/llm-coding-tools-serdesai)

Lightweight, high-performance serdesAI framework Tool implementations for coding tools.

## Features

- **File operations** - Read, write, edit, glob, grep with two access modes:
  - `absolute::*` - Unrestricted filesystem access
  - `allowed::*` - Sandboxed to configured directories
- **Shell execution** - Cross-platform command execution with timeout
- **Web fetching** - URL content retrieval with format conversion
- **Task delegation** - Sub-agent spawning for complex tasks
- **Todo management** - Shared-state todo list tracking
- **Context strings** - LLM guidance text for tool usage (re-exported from core)
- **Schema builders** - Composable helpers for custom tool definitions

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
llm-coding-tools-serdesai = "0.1"
```

## Quick Start

Minimal runnable agent (requires `OPENAI_API_KEY`):

```rust,no_run
use llm_coding_tools_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use llm_coding_tools_serdesai::{BashTool, PreambleBuilder, create_todo_tools};
use serdes_ai::prelude::*;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (todo_read, todo_write, _state) = create_todo_tools();
    let mut pb = PreambleBuilder::<false>::new();

    // Build agent with tools - call .system_prompt() last
    let agent = AgentBuilder::<(), String>::from_model("openai:gpt-4o")?
        .tool(pb.track(ReadTool::<true>::new()))
        .tool(pb.track(GlobTool::new()))
        .tool(pb.track(GrepTool::<true>::new()))
        .tool(pb.track(BashTool::new()))
        .tool(pb.track(todo_read))
        .tool(pb.track(todo_write))
        .system_prompt(pb.build())  // Last, after tracking all tools
        .build();

    // Run agent with tools
    let response = agent
        .run("Search for TODO comments in src/", ())
        .await?;
    println!("{}", response.output());

    Ok(())
}
```

See the [serdesai-basic example](examples/serdesai-basic.rs) for a complete working setup.

## Usage

File tools come in `absolute::*` (unrestricted) and `allowed::*` (sandboxed) variants:

```rust,no_run
use llm_coding_tools_serdesai::absolute::{ReadTool, WriteTool};
use llm_coding_tools_serdesai::allowed::{ReadTool as AllowedReadTool, WriteTool as AllowedWriteTool};
use std::path::PathBuf;

// Unrestricted access (absolute paths)
let read = ReadTool::<true>::new();

// Sandboxed access (paths relative to allowed directories)
let allowed_paths = vec![PathBuf::from("/home/user/project"), PathBuf::from("/tmp")];
let sandboxed_read: AllowedReadTool<true> = AllowedReadTool::new(allowed_paths.clone()).unwrap();
let sandboxed_write = AllowedWriteTool::new(allowed_paths).unwrap();
```

Other tools: `BashTool`, `WebFetchTool`, `TaskTool`, `TodoReadTool`, `TodoWriteTool`.
Use `PreambleBuilder` to track tools and pass `pb.build()` to `.system_prompt()`.
Use `AgentBuilderExt::tool()` to add tools that implement `Tool<Deps>` to the agent.
Context strings are re-exported in `llm_coding_tools_serdesai::context` (e.g., `BASH`, `READ_ABSOLUTE`).

## Examples

```bash
# Basic agent setup with AgentBuilderExt
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai
```

## License

Apache 2.0
