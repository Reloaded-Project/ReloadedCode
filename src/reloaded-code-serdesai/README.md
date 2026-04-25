# reloaded-code-serdesai

[![Crates.io](https://img.shields.io/crates/v/reloaded-code-serdesai.svg)](https://crates.io/crates/reloaded-code-serdesai) [![Docs.rs](https://docs.rs/reloaded-code-serdesai/badge.svg)](https://docs.rs/reloaded-code-serdesai)

Ready-to-use [SerdesAI] integration for [reloaded-code]. Tool adapters,
agent build context, 15 provider bridges, and multi-agent task delegation.

[Documentation] · [API Reference]

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
reloaded-code-serdesai = "0.2"
```

## Quick Start

Minimal runnable agent (requires `OPENAI_API_KEY`).

```rust,no_run
use reloaded_code_serdesai::{ReadTool, GlobTool, GrepTool, EditTool, AbsolutePathResolver};
use reloaded_code_serdesai::agent_ext::AgentBuilderExt;
use reloaded_code_serdesai::{BashTool, SystemPromptBuilder, WebFetchTool, create_todo_tools};
use serdes_ai::prelude::*;

# #[tokio::main]
# async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
let (todo_read, todo_write, _state) = create_todo_tools();
let mut pb = SystemPromptBuilder::new();

// Build agent with tools - call .system_prompt() last
let agent = AgentBuilder::<(), String>::from_model("openai:gpt-5.4")?
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
use reloaded_code_serdesai::{ReadTool, WriteTool, AbsolutePathResolver};

// Unrestricted access with absolute paths
let read = ReadTool::new(AbsolutePathResolver);
let write = WriteTool::new(AbsolutePathResolver);
```

### Sandboxed File Access

Restrict file operations to specific directories using [`AllowedPathResolver`]:

```rust,no_run
use reloaded_code_serdesai::{ReadTool, WriteTool, EditTool, AllowedPathResolver};
use std::path::PathBuf;

let allowed_paths = vec![PathBuf::from("/home/user/project"), PathBuf::from("/tmp")];
let resolver = AllowedPathResolver::new(allowed_paths).unwrap();

let read = ReadTool::new(resolver.clone());
let write = WriteTool::new(resolver.clone());
let edit = EditTool::new(resolver);
```

For fine-grained glob-based allow/deny rules, use [`AllowedGlobResolver`]:

```rust,no_run
use reloaded_code_serdesai::ReadTool;
use reloaded_code_core::path::{AllowedGlobResolver, GlobPolicy, RuleAction};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let resolver = AllowedGlobResolver::new("/home/user/project")?
    .with_policy(
        GlobPolicy::builder()
            .add("src/**", RuleAction::Allow)?
            .add("target/**", RuleAction::Deny)?
            .build()?
    );
let read = ReadTool::new(resolver);
# Ok(())
# }
```

Use `SystemPromptBuilder` to track tools and generate context-aware prompts.
Context strings are re-exported in `reloaded_code_serdesai::context`
(e.g., `BASH`, `READ_ABSOLUTE`, `READ_ALLOWED`).

## Build and Run Agents

Load agents, load the [models.dev] catalog, then build by name from a shared
[`AgentBuildContext`]:

```rust,no_run
use reloaded_code_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use reloaded_code_core::CredentialResolver;
use reloaded_code_models_dev::ModelsDevCatalog;
use reloaded_code_serdesai::{AgentBuildContext, AgentDefaults};
use std::{path::PathBuf, sync::Arc};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let agents_dir = PathBuf::from("path/to/your/agents");
let mut catalog = AgentCatalog::new();
AgentLoader::new().add_directory(&mut catalog, &agents_dir)?;

let load_result = ModelsDevCatalog::load().await?;

let runtime = AgentRuntimeBuilder::new()
    .catalog(catalog)
    .defaults(AgentDefaults::with_model("ollama-cloud/minimax-m2.7"))
    // .max_task_depth(5) // Optional: defaults to 3 Task hops
    .build()?;

let build_context = AgentBuildContext::new(
    Arc::new(runtime),
    Arc::new(load_result.catalog),
    Arc::new(CredentialResolver::new()),
    Arc::from(reloaded_code_core::resolve_workspace_root()?.as_path()),
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

Sandboxing is **not enabled by default** for the `bash` tool - it runs
unsandboxed on the host unless you explicitly configure a bubblewrap profile.
File tools are sandboxed to the workspace root by default.

Enable the `linux-bubblewrap` feature flag to use Linux `bwrap` sandbox profiles:

```toml
[dependencies]
reloaded-code-serdesai = { version = "0.2", features = ["linux-bubblewrap"] }
```

Two profiles are available:

- **Public Bot**: Assumes anyone can call; and thus defaults to the strictest containment.
    - No full host filesystem access, synthetic home, memory-backed `/tmp`, network disabled, sanitized system `PATH`.
- **Trusted Maintenance**: Assumes work in a more trusted environment, e.g. maintaining codebases.
    - Read-only host `/` with writable overlays, disk-backed `/tmp`, sanitized host `PATH`, network enabled.

We default to **Public Bot** profile when sandboxing is used.
In either case, trusted or not, please evaluate whether the solution fits your
security needs. I can make no guarantees.

More info in [SANDBOX-PROFILES.md](https://github.com/Reloaded-Project/ReloadedCode/blob/main/SANDBOX-PROFILES.md).

## Examples

```bash
# Basic agent setup with AgentBuilderExt
cargo run --example serdesai-basic -p reloaded-code-serdesai

# Sandboxed file access with allowed::* tools
cargo run --example serdesai-sandboxed -p reloaded-code-serdesai

# Execution with Sandboxed `bash`
cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p reloaded-code-serdesai

# Markdown agent runtime (shared build context)
cargo run --example serdesai-agents -p reloaded-code-serdesai

# Stateless single-hop Task delegation
cargo run --example serdesai-task -p reloaded-code-serdesai
```

For agent runtime architecture, see [AGENTS-ARCHITECTURE.md](AGENTS-ARCHITECTURE.md).

## License

Apache 2.0

[reloaded-code]: https://github.com/Reloaded-Project/ReloadedCode
[SerdesAI]: https://crates.io/crates/serdes-ai
[models.dev]: https://models.dev
[Documentation]: https://reloaded-project.github.io/ReloadedCode/
[API Reference]: https://docs.rs/reloaded-code-serdesai
