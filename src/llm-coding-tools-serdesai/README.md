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

**Note**: The `default_tools` function (defined in `examples/serdesai-agents.rs`) returns cloneable `ToolCatalogEntry` items that can be reused for building multiple agents. The `AgentRegistryBuilder` uses these to construct tool descriptions and filter based on agent permissions. The `deps` parameter is passed to registry agents at invocation time.

### Other Tools

The following tools are available for use with agents:

- `BashTool` - Execute shell commands
- `WebFetchTool` - Fetch content from URLs
- `TodoReadTool` / `TodoWriteTool` - Manage todo items

Use `SystemPromptBuilder` to track tools and populate the environment section:

```rust,ignore
use llm_coding_tools_serdesai::SystemPromptBuilder;

let pb = SystemPromptBuilder::new()
    .working_directory(std::env::current_dir()?);
agent_builder.system_prompt(pb.build());
```

Add tools to agents using `AgentBuilderExt::tool()`:

```rust,ignore
agent_builder.tool(MyTool::new());
```

Context strings (e.g., `BASH`, `READ_ABSOLUTE`) are re-exported in `llm_coding_tools_serdesai::context`.

### models.dev Resolver

Use the models.dev catalog to resolve per-provider API keys/base URLs:

```rust,no_run
# use std::env;
# use llm_coding_tools_models_dev::ModelsDevCatalog;
# use llm_coding_tools_serdesai::{AgentDefaults, ModelsDevResolver, ProviderOverride, ProviderOverrides};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let catalog = ModelsDevCatalog::load_shared_cache_or_bundled()?.catalog;
let overrides = ProviderOverrides::new().insert_override(
    "openai",
    ProviderOverride { api_key: Some(env::var("OPENAI_API_KEY")?), base_url: None, endpoint_env: None },
);
let resolver = ModelsDevResolver::new(Some(catalog), overrides.clone());

let defaults = AgentDefaults {
    model: "openai:gpt-4o".into(),
    model_resolver: Some(resolver),
    provider_overrides: overrides,
    api_key: None,
    base_url: None,
    temperature: None,
    top_p: None,
    options: Default::default(),
};
# Ok(())
# }
```

**OpenAI-compatible providers**: serdesAI does not infer providers from base URLs. Use an `openai:` model spec and set a provider-specific `base_url` via overrides.

**Reasoning models**: If you need `OpenAIResponsesModel` for `o1`, `o3`, or `gpt-5`, construct it directly instead of using `ModelConfig`.

**OpenRouter/HuggingFace**: `build_model_with_config` does not support these providers; use `OpenRouterModel::new` or `HuggingFaceModel::new` directly.
OpenRouter does not support base URL overrides; resolver should not surface `base_url` for this provider.

**Resolver fallback behavior**: When no resolver is provided, the registry attempts to load the models.dev catalog from the shared cache or bundled snapshot. If that fails, it falls back to an empty catalog (meaning only explicit specs are usable and no provider mapping occurs).


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
