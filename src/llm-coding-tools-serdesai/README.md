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
- **Todo management** - Shared-state todo list tracking
- **Task tool** - Registry-driven agent invocation with permission checks
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
use llm_coding_tools_serdesai::{BashTool, SystemPromptBuilder, create_todo_tools};
use serdes_ai::prelude::*;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (todo_read, todo_write, _state) = create_todo_tools();
    let mut pb = SystemPromptBuilder::new();

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
use llm_coding_tools_serdesai::AllowedPathResolver;
use std::path::PathBuf;

// Unrestricted access (absolute paths)
let read = ReadTool::<true>::new();

// Sandboxed access (paths relative to allowed directories)
let allowed_paths = vec![PathBuf::from("/home/user/project"), PathBuf::from("/tmp")];
let resolver = AllowedPathResolver::new(allowed_paths).unwrap();
let sandboxed_read: AllowedReadTool<true> = AllowedReadTool::new(resolver.clone());
let sandboxed_write = AllowedWriteTool::new(resolver);
```

### Task Tool (Registry-Driven)

The Task tool allows agents to invoke other agents via a registry-based lookup.

**Note**: For a complete runnable example, see `examples/serdesai-agents.rs`.

Setup requires three steps:

1. **Load agent configs** into `AgentCatalog`
2. **Build a serdesAI registry** with `AgentRegistryBuilder` and tools
3. **Create `TaskTool`** with registry, permissions, and deps

The example file shows the complete setup.

**Note**: The `default_tools` function returns cloneable `ToolCatalogEntry` items that can be reused for building multiple agents. The `AgentRegistryBuilder` uses these to construct tool descriptions and filter based on agent permissions. The `deps` parameter is passed to registry agents at invocation time.

Other tools: `BashTool`, `WebFetchTool`, `TodoReadTool`, `TodoWriteTool`.
Use `SystemPromptBuilder` to track tools and pass `pb.build()` to `.system_prompt()`. Set `working_directory()` so the environment section is populated.
Use `AgentBuilderExt::tool()` to add tools that implement `Tool<Deps>` to the agent.
Context strings are re-exported in `llm_coding_tools_serdesai::context` (e.g., `BASH`, `READ_ABSOLUTE`).

### Migration from Legacy Task APIs

The previous task setup using `TaskToolCore` and `SubagentRegistry` has been replaced with the registry-driven flow. Key changes:

| Legacy | New |
|--------|-----|
| `SubagentRegistry` from agents crate | `AgentCatalog` + serdesAI `AgentRegistry` |
| `TaskToolCore` | `TaskTool` (registry-based implementation) |
| Manually building agents | `AgentRegistryBuilder` builds all at once |

For a detailed migration example, see `examples/serdesai-agents.rs`.

## Examples

```bash
# Basic agent setup with AgentBuilderExt
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai
```

## License

Apache 2.0
