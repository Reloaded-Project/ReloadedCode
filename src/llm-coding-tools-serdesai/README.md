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

```rust
use llm_coding_tools_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_serdesai::{BashTool, PreambleBuilder, create_todo_tools};
use serdes_ai::tools::ToolRegistry;

#[tokio::main]
async fn main() {
    let mut pb = PreambleBuilder::<false>::new();
    let mut registry = ToolRegistry::<()>::new();

    registry.register(pb.track(ReadTool::<true>::new()));
    registry.register(pb.track(GlobTool::new()));
    registry.register(pb.track(GrepTool::<true>::new()));
    registry.register(pb.track(BashTool::new()));

    let (todo_read, todo_write, _state) = create_todo_tools();
    registry.register(pb.track(todo_read));
    registry.register(pb.track(todo_write));

    let preamble = pb.build();

    // Pass `preamble` to your agent's system prompt
    // Pass `registry` to your agent's tools
}
```

See the [basic example](examples/basic.rs) for a complete working setup.

## Usage

File tools come in `absolute::*` (unrestricted) and `allowed::*` (sandboxed) variants:

```rust
use llm_coding_tools_serdesai::absolute::{ReadTool, WriteTool};
use llm_coding_tools_serdesai::allowed::{ReadTool as AllowedReadTool, WriteTool as AllowedWriteTool};
use std::path::PathBuf;

// Unrestricted access (absolute paths)
let read = ReadTool::<true>::new();

// Sandboxed access (paths relative to allowed directories)
let allowed_paths = vec![PathBuf::from("/home/user/project"), PathBuf::from("/tmp")];
let sandboxed_read: AllowedReadTool<true> = AllowedReadTool::new(allowed_paths.clone());
let sandboxed_write = AllowedWriteTool::new(allowed_paths);
```

Other tools: `BashTool`, `WebFetchTool`, `TaskTool`, `TodoReadTool`, `TodoWriteTool`.
Use `PreambleBuilder` to register tools and pass `pb.build()` to your agent's system prompt.
Context strings are re-exported in `llm_coding_tools_serdesai::context` (e.g., `BASH`, `READ_ABSOLUTE`).

## Examples

```bash
# Basic toolset setup with PreambleBuilder
cargo run --example basic -p llm-coding-tools-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example sandboxed -p llm-coding-tools-serdesai
```

## License

Apache 2.0
