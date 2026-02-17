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

Minimal runnable registry-backed agent (requires `SYNTHETIC_API_KEY`):

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader};
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, AllowedPathResolver, ProviderOverrides, TodoState,
    default_tools,
};
use serdes_ai::prelude::*;
use std::sync::Arc;

const MODEL_SPEC: &str = "synthetic/hf:zai-org/GLM-4.7";
const FILE_READER: &str = r#"---
name: file-reader
mode: subagent
description: Example subagent
permission:
  read: allow
  glob: allow
---
Respond concisely.
"#;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut catalog = AgentCatalog::new();
    AgentLoader::new().add_from_str(&mut catalog, FILE_READER, "file-reader")?;

    let allowed_paths =
        AllowedPathResolver::new([std::env::current_dir()?, std::env::temp_dir()])?;
    let tools = default_tools(true, Some(allowed_paths), TodoState::new());

    let defaults = AgentDefaults {
        model: MODEL_SPEC.to_string(),
        model_resolver: None,
        provider_overrides: ProviderOverrides::new(),
        api_key: None,
        base_url: None,
        temperature: None,
        top_p: None,
        options: Default::default(),
    };

    let deps = Arc::new(());
    let registry = AgentRegistryBuilder::<()>::new(defaults, tools)
        .build_with_recursive_task(&catalog, Arc::clone(&deps))?;
    let primary = registry
        .get("file-reader")
        .ok_or_else(|| std::io::Error::other("missing file-reader agent"))?;

    let response = primary
        .agent
        .run("List Rust files in the current directory.", deps)
        .await?;
    println!("{}", response.output());

    Ok(())
}
```

See the [serdesai-agents example](examples/serdesai-agents.rs) for a complete working setup.

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
2. **Build tool catalog** with `default_tools`
3. **Build recursive registry** with `AgentRegistryBuilder::build_with_recursive_task`

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader};
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, ProviderOverrides, TodoState, default_tools,
};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loader = AgentLoader::new();
    let mut catalog = AgentCatalog::new();
    loader.add_file(&mut catalog, "agents/example.md")?;

    let defaults = AgentDefaults {
        model: "synthetic/hf:zai-org/GLM-4.7".to_string(),
        model_resolver: None,
        provider_overrides: ProviderOverrides::new(),
        api_key: None,
        base_url: None,
        temperature: None,
        top_p: None,
        options: Default::default(),
    };

    let tools = default_tools(true, None, TodoState::new());
    let deps = Arc::new(());
    let _registry = AgentRegistryBuilder::<()>::new(defaults, tools)
        .build_with_recursive_task(&catalog, deps)?;

    Ok(())
}
```

`default_tools` returns cloneable `ToolCatalogEntry` items. `AgentRegistryBuilder` applies permission filtering per agent and wires `Task` automatically when `permission.task` allows delegation.

### Other Tools

The following tools are available for use with agents:

- `BashTool` - Execute shell commands
- `WebFetchTool` - Fetch content from URLs
- `TodoReadTool` / `TodoWriteTool` - Manage todo items

Use `SystemPromptBuilder` to track tools and populate the environment section:

```rust,ignore
use llm_coding_tools_serdesai::SystemPromptBuilder;

let mut pb = SystemPromptBuilder::new()
    .working_directory(std::env::current_dir()?);
// ... track tools with pb.track() ...
// Finally set the system prompt:
let agent = AgentBuilder::from_model("synthetic/hf:zai-org/GLM-4.7")?
    .system_prompt(pb.build())
    .build()?;
```

Add tools to agents using `AgentBuilderExt::tool()`:

```rust,ignore
let agent = AgentBuilder::from_model("synthetic/hf:zai-org/GLM-4.7")?
    .tool(MyTool::new())
    .build()?;
```

Context strings (e.g., `BASH`, `READ_ABSOLUTE`) are re-exported in `llm_coding_tools_serdesai::context`.

### Model Resolver

Registry builds always resolve models through `AgentDefaults.model_resolver`.

Recommended default (`model_resolver: None`):

```rust,no_run
# use llm_coding_tools_serdesai::{AgentDefaults, ProviderOverrides};
let defaults = AgentDefaults {
    model: "synthetic/hf:zai-org/GLM-4.7".into(),
    model_resolver: None,
    provider_overrides: ProviderOverrides::new(),
    api_key: None,
    base_url: None,
    temperature: None,
    top_p: None,
    options: Default::default(),
};
```

This uses the default resolver abstraction, which is models.dev-backed today and can be replaced by injecting your own `Arc<dyn ModelResolver + Send + Sync>`.

Manual openai-compatible endpoint override fallback:

```rust,no_run
# use llm_coding_tools_serdesai::{AgentDefaults, ProviderOverride, ProviderOverrides};
let overrides = ProviderOverrides::new().insert_override(
    "synthetic",
    ProviderOverride {
        api_key: None,
        base_url: Some("https://your-openai-compatible-endpoint/v1".into()),
        endpoint_env: None,
    },
);

let defaults = AgentDefaults {
    model: "synthetic/hf:zai-org/GLM-4.7".into(),
    model_resolver: None,
    provider_overrides: overrides,
    api_key: None,
    base_url: None,
    temperature: None,
    top_p: None,
    options: Default::default(),
};
```

**OpenCode model specs**: use `<provider>/<model>` in agent/frontmatter configuration (for example `synthetic/hf:zai-org/GLM-4.7`). Resolver preserves the original spec and infers runtime provider family from provider metadata.

**OpenAI-compatible providers**: keep provider identity in the user spec (for example `synthetic/hf:zai-org/GLM-4.7`); resolver derives openai-compatible runtime behavior and resolves provider-specific endpoint settings.

**Resolver fallback behavior**: when no resolver is injected, registry load uses shared-cache or bundled catalog data. If catalog loading fails, resolution falls back to explicit raw spec behavior.


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
# Registry + resolver flow (recommended)
SYNTHETIC_API_KEY=... cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# Basic agent setup with AgentBuilderExt
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai
```

## License

Apache 2.0
