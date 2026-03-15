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

## Tool Variants

File tools come in two variants with identical APIs:

- **`absolute::*`** - Unrestricted filesystem access using absolute paths
- **`allowed::*`** - Sandboxed to configured directories via `AllowedPathResolver`

```rust,no_run
use llm_coding_tools_serdesai::absolute::{ReadTool, WriteTool};
use llm_coding_tools_serdesai::allowed::{ReadTool as AllowedReadTool, WriteTool as AllowedWriteTool};
use llm_coding_tools_serdesai::AllowedPathResolver;
use std::path::PathBuf;

// Unrestricted access
let read = ReadTool::<true>::new();

// Sandboxed access
let allowed_paths = vec![PathBuf::from("/home/user/project"), PathBuf::from("/tmp")];
let resolver = AllowedPathResolver::new(allowed_paths).unwrap();
let sandboxed_read: AllowedReadTool<true> = AllowedReadTool::new(resolver.clone());
let sandboxed_write = AllowedWriteTool::new(resolver);
```

Other tools: `BashTool`, `WebFetchTool`, `TodoReadTool`, `TodoWriteTool`.

Use `SystemPromptBuilder` to track tools and generate context-aware prompts. Context strings are re-exported in `llm_coding_tools_serdesai::context` (e.g., `BASH`, `READ_ABSOLUTE`).

## Agent Runtime

For OpenCode-style agent support, use `AgentRuntimeExt` to build agents from an [`AgentRuntime`](https://docs.rs/llm-coding-tools-agents/latest/llm_coding_tools_agents/struct.AgentRuntime.html):

```rust,no_run
use llm_coding_tools_serdesai::AgentRuntimeExt;
use llm_coding_tools_agents::AgentRuntimeBuilder;
use llm_coding_tools_core::{CredentialResolver, models::ModelCatalog};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# fn get_catalog() -> ModelCatalog { unimplemented!() }
let runtime = AgentRuntimeBuilder::new().build();
let catalog = get_catalog(); // Load from models-dev, config file, etc.
let credentials = CredentialResolver::new();
let _agent = runtime.build("planner", &catalog, &credentials)?;
# Ok(())
# }
```

This requires the `llm-coding-tools-agents` crate, a `ModelCatalog` for model resolution, and an application-owned credential resolver.

### Task Tool

Use [`AgentRuntimeTaskExt::build_with_task`] to build an agent that can delegate one-shot work to subagents via the Task tool.

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentDefaults, AgentRuntimeTaskExt};
use std::{path::PathBuf, sync::Arc};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let examples_root = PathBuf::from("/path/to/your/project/examples");
let load_result = ModelsDevCatalog::load().await?;

let mut catalog = AgentCatalog::new();
AgentLoader::new().add_directory(&mut catalog, &examples_root)?;

let runtime = AgentRuntimeBuilder::new()
    .catalog(catalog)
    .defaults(AgentDefaults::with_model("synthetic/hf:zai-org/GLM-4.7"))
    .build();

let credentials = Arc::new(CredentialResolver::new());
let agent = runtime.build_with_task(
    "orchestrator",
    Arc::new(load_result.catalog),
    credentials,
)?;
# Ok(())
# }
```

Each Task call builds and runs the subagent once; `session_id` is rejected. Use [`build_agent_with_credentials_and_task`] for the lower-level helper. See [examples/serdesai-task.rs](examples/serdesai-task.rs).

## Examples

```bash
# Basic agent setup with AgentBuilderExt
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# Markdown agent runtime (no delegation)
cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# Stateless single-hop Task delegation
cargo run --example serdesai-task -p llm-coding-tools-serdesai
```

## License

Apache 2.0
