# llm-coding-tools-serdesai

[![Crates.io](https://img.shields.io/crates/v/llm-coding-tools-serdesai.svg)](https://crates.io/crates/llm-coding-tools-serdesai) [![Docs.rs](https://docs.rs/llm-coding-tools-serdesai/badge.svg)](https://docs.rs/llm-coding-tools-serdesai)

Lightweight, high-performance serdesAI implementation for [llm-coding-tools].

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
llm-coding-tools-serdesai = "0.2"
```

## Quick Start

Minimal runnable agent (requires `OPENAI_API_KEY`).

```rust,no_run
use llm_coding_tools_serdesai::{ReadTool, GlobTool, GrepTool, EditTool, AbsolutePathResolver};
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
use llm_coding_tools_serdesai::{BashTool, SystemPromptBuilder, WebFetchTool, create_todo_tools};
use serdes_ai::prelude::*;

# #[tokio::main]
# async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
let (todo_read, todo_write, _state) = create_todo_tools();
let mut pb = SystemPromptBuilder::new();

// Build agent with tools - call .system_prompt() last
let agent = AgentBuilder::<(), String>::from_model("openai:gpt-4o")?
    .tool(pb.track(ReadTool::new(AbsolutePathResolver)))
    .tool(pb.track(GlobTool::new(AbsolutePathResolver)))
    .tool(pb.track(GrepTool::new(AbsolutePathResolver)))
    .tool(pb.track(EditTool::new(AbsolutePathResolver)))
    .tool(pb.track(BashTool::host()))
    .tool(pb.track(WebFetchTool::new()))
    .tool(pb.track(todo_read))
    .tool(pb.track(todo_write))
    .system_prompt(pb.build())  // Last, after tracking all tools
    .build();

// Run agent with tools
let response = agent
    .run("Search for TODO comments in src/", ())
    .await?;
println!("{}", response.output());

# Ok(())
# }
```

See the [serdesai-basic example](examples/serdesai-basic.rs) for a complete working setup.

For named agents and subagent Task delegation, see [Build and Run Agents](#build-and-run-agents).

## File Tools

File tools work with any [`PathResolver`] implementation:
- [`AbsolutePathResolver`] - Unrestricted filesystem access using absolute paths
- [`AllowedPathResolver`] - Sandboxed to configured directories

```rust,no_run
use llm_coding_tools_serdesai::{ReadTool, WriteTool, AbsolutePathResolver};

// Unrestricted access with absolute paths
let read = ReadTool::new(AbsolutePathResolver);
let write = WriteTool::new(AbsolutePathResolver);
```

### Sandboxed File Access

Restrict file operations to specific directories using [`AllowedPathResolver`]:

```rust,no_run
use llm_coding_tools_serdesai::{ReadTool, WriteTool, EditTool, AllowedPathResolver};
use std::path::PathBuf;

let allowed_paths = vec![PathBuf::from("/home/user/project"), PathBuf::from("/tmp")];
let resolver = AllowedPathResolver::new(allowed_paths).unwrap();

let read = ReadTool::new(resolver.clone());
let write = WriteTool::new(resolver.clone());
let edit = EditTool::new(resolver);
```

Use `SystemPromptBuilder` to track tools and generate context-aware prompts.
Context strings are re-exported in `llm_coding_tools_serdesai::context`
(e.g., `BASH`, `READ_ABSOLUTE`, `READ_ALLOWED`).

## Build and Run Agents

Load agents, load the models.dev catalog, then build by name from a shared
[`AgentBuildContext`]:

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentBuildContext, AgentDefaults};
use std::{path::PathBuf, sync::Arc};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let agents_dir = PathBuf::from("path/to/your/agents");
let mut catalog = AgentCatalog::new();
AgentLoader::new().add_directory(&mut catalog, &agents_dir)?;

let load_result = ModelsDevCatalog::load().await?;

let runtime = AgentRuntimeBuilder::new()
    .catalog(catalog)
    .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4o-mini"))
    // .max_task_depth(5) // Optional: defaults to 3 Task hops
    .build()?;

let build_context = AgentBuildContext::new(
    Arc::new(runtime),
    Arc::new(load_result.catalog),
    Arc::new(CredentialResolver::new()),
);
let agent = build_context.build("planner")?;
let response = agent.run("Say hello in one sentence.", ()).await?;
println!("{}", response.output());
# Ok(())
# }
```

`AgentRuntimeBuilder::new().build()` is empty by default, so load agents into
`.catalog(...)` before `build_context.build("planner")?`.

Task uses the same setup and `build()` call; the `task` tool is attached
automatically when callable targets exist and `max_task_depth` allows delegation.

If you already have your own `ModelCatalog`, you can use that instead of
`ModelsDevCatalog::load()` (for example via a `get_catalog()` helper).

See [examples/serdesai-agents.rs](examples/serdesai-agents.rs) and
[examples/serdesai-task.rs](examples/serdesai-task.rs).

## Linux Shell Sandboxing

Enable the `linux-bubblewrap` feature flag to use Linux `bwrap` sandbox profiles:

```toml
[dependencies]
llm-coding-tools-serdesai = { version = "0.2", features = ["linux-bubblewrap"] }
```

Out of the box, 2 profiles are available:

- **Public Bot**: Assumes anyone can call; and thus defaults to the strictest containment.
    - No full host filesystem access, synthetic home, memory-backed `/tmp`, network disabled, sanitized system `PATH`.
- **Trusted Maintenance**: Assumes work in a more trusted environment, e.g. maintaining codebases.
    - Read-only host `/` with writable overlays, disk-backed `/tmp`, sanitized host `PATH`, network enabled.

We default to **Public Bot** profile when sandboxing is used.
In either case, trusted or not, please evaluate whether the solution fits your
security needs. I can make no guarantees.

More info in [SANDBOX-PROFILES.md](https://github.com/Sewer56/llm-coding-tools/blob/main/SANDBOX-PROFILES.md).

## Examples

```bash
# Basic agent setup with AgentBuilderExt
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# Execution with Sandboxed `bash`
cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai

# Markdown agent runtime (shared build context)
cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# Stateless single-hop Task delegation
cargo run --example serdesai-task -p llm-coding-tools-serdesai
```

For agent runtime architecture, see [AGENTS-ARCHITECTURE.md](AGENTS-ARCHITECTURE.md).

## License

Apache 2.0

[llm-coding-tools]: https://github.com/Sewer56/llm-coding-tools
