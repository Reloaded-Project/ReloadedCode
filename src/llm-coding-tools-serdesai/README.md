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

Minimal runnable agent (requires `OPENAI_API_KEY` for synthetic API):

```rust,no_run
use llm_coding_tools_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use llm_coding_tools_serdesai::{BashTool, SystemPromptBuilder, create_todo_tools};
use serdes_ai::models::openai::OpenAIChatModel;
use serdes_ai::prelude::*;

const OPENAI_API_KEY: &str = "";
const OPENAI_MODEL: &str = "hf:zai-org/GLM-4.7";
const OPENAI_BASE_URL: &str = "https://api.synthetic.new/openai/v1";

fn get_openai_api_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
        if OPENAI_API_KEY.is_empty() {
            panic!("OPENAI_API_KEY environment variable must be set");
        }
        OPENAI_API_KEY.to_string()
    })
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let (todo_read, todo_write, _state) = create_todo_tools();
    let mut pb = SystemPromptBuilder::new();

    // Build agent with tools - call .system_prompt() last
    let model = OpenAIChatModel::new(OPENAI_MODEL, get_openai_api_key())
        .with_base_url(OPENAI_BASE_URL);
    let agent = AgentBuilder::<(), String>::new(model)
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

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader};
use llm_coding_tools_core::permissions::Ruleset;
use llm_coding_tools_serdesai::{
    AgentDefaults, AgentRegistryBuilder, TaskTool, TaskDefinitionSnapshot,
    TaskTargetSummary, default_tools, ProviderOverrides, TodoState, TaskRegistryHandle,
};
use std::sync::Arc;

const OPENAI_API_KEY: &str = "";
const OPENAI_BASE_URL: &str = "https://api.synthetic.new/openai/v1";

fn get_openai_api_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| {
        if OPENAI_API_KEY.is_empty() {
            panic!("OPENAI_API_KEY environment variable must be set");
        }
        OPENAI_API_KEY.to_string()
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load agent configs
    let loader = AgentLoader::new();
    let mut catalog = AgentCatalog::new();
    loader.add_file(&mut catalog, "agents/example.md")?;

    // 2. Build registry with defaults and tools
    let defaults = AgentDefaults {
        model: "openai:hf:zai-org/GLM-4.7".to_string(),
        model_resolver: None,
        provider_overrides: ProviderOverrides::new(),
        api_key: Some(get_openai_api_key()),
        base_url: Some(OPENAI_BASE_URL.to_string()),
        temperature: None,
        top_p: None,
        options: Default::default(),
    };

    let tools = default_tools(true, None, TodoState::new());
    let registry = Arc::new(AgentRegistryBuilder::new(defaults, tools)
        .build(&catalog)?);

    // 3. Create TaskTool with registry handle, permissions, and deps
    let registry_handle = Arc::new(TaskRegistryHandle::from_registry(Arc::clone(&registry)));
    let snapshot = TaskDefinitionSnapshot {
        targets: registry.iter().map(|(name, entry)| TaskTargetSummary {
            name: name.clone(),
            mode: entry.config.mode,
            tool_names: entry.tool_names.clone(),
        }).collect(),
    };
    let rules = Ruleset::new(); // Configure permissions as needed
    let deps = Arc::new(());

    let task_tool = TaskTool::for_registry_caller(
        registry_handle,
        "primary-agent",
        rules,
        snapshot,
        deps,
    );

    Ok(())
}
```

**Note**: The `default_tools` function (defined in `examples/serdesai-agents.rs`) returns cloneable `ToolCatalogEntry` items that can be reused for building multiple agents. The `AgentRegistryBuilder` uses these to construct tool descriptions and filter based on agent permissions. The `deps` parameter is passed to registry agents at invocation time.

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
let agent = AgentBuilder::from_model("openai:gpt-4o")?
    .system_prompt(pb.build())
    .build()?;
```

Add tools to agents using `AgentBuilderExt::tool()`:

```rust,ignore
let agent = AgentBuilder::from_model("openai:gpt-4o")?
    .tool(MyTool::new())
    .build()?;
```

Context strings (e.g., `BASH`, `READ_ABSOLUTE`) are re-exported in `llm_coding_tools_serdesai::context`.

### models.dev Resolver

Use the models.dev catalog to resolve per-provider API keys/base URLs:

```rust,no_run
# use std::env;
# use std::sync::Arc;
# use llm_coding_tools_models_dev::ModelsDevCatalog;
# use llm_coding_tools_serdesai::{AgentDefaults, ModelsDevResolver, ProviderOverride, ProviderOverrides};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let catalog = ModelsDevCatalog::load_shared_cache_or_bundled()?.catalog;
let overrides = ProviderOverrides::new().insert_override(
    "openai",
    ProviderOverride {
        api_key: Some(env::var("OPENAI_API_KEY")?),
        base_url: Some("https://api.synthetic.new/openai/v1".into()),
        endpoint_env: None
    },
);
let resolver = ModelsDevResolver::new(Some(catalog), overrides.clone());

let defaults = AgentDefaults {
    model: "synthetic/hf:zai-org/GLM-4.7".into(),
    model_resolver: Some(Arc::new(resolver)),
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

**OpenCode model specs**: use `<provider>/<model>` in agent/frontmatter configuration (for example `synthetic/hf:zai-org/GLM-4.7`). Resolver preserves the original spec and infers runtime provider family from models.dev `provider.npm`.

**OpenAI-compatible providers**: keep provider identity in the user spec (for example `router/m1`); resolver derives openai-compatible runtime behavior from `@ai-sdk/openai-compatible` metadata and resolves provider-specific base URL settings.

**Reasoning models**: If you need `OpenAIResponsesModel` for `o1`, `o3`, or `gpt-5`, construct it directly instead of using `ModelConfig`.

**OpenRouter/HuggingFace**: `build_model_with_config` does not support these providers; use `OpenRouterModel::new` or `HuggingFaceModel::new` directly.
OpenRouter does not support base URL overrides; resolver should not surface `base_url` for this provider.

**Resolver fallback behavior**: When no resolver is provided, the registry attempts to load the models.dev catalog from the shared cache or bundled snapshot. If that fails, it falls back to an empty catalog (meaning only explicit specs are usable and no provider mapping occurs).

**Custom resolver injection**: Pass any `Arc<dyn ModelResolver + Send + Sync>` in `AgentDefaults.model_resolver` to bypass models.dev-specific resolution while preserving the same registry build flow.


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
