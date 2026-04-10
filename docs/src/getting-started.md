# Getting Started

This guide walks you through setting up your first coding agent with
llm-coding-tools. It assumes you have a Rust project and an LLM API key
(e.g. `OPENAI_API_KEY`).

## Add the dependency

Pick the crate that matches your use case:

| Use case                                                  | Crate                         |
| --------------------------------------------------------- | ----------------------------- |
| I want a ready-to-use agent with the [SerdesAI] framework | `llm-coding-tools-serdesai`   |
| I want to build my own framework integration              | `llm-coding-tools-core`       |
| I need agent markdown file loading                        | `llm-coding-tools-agents`     |
| I need the [models.dev] model catalog                     | `llm-coding-tools-models-dev` |
| I need Linux shell sandboxing                             | `llm-coding-tools-bubblewrap` |

## Build your first agent

!!! info "Agents are defined as markdown files with YAML frontmatter"
    The agent file format mirrors [OpenCode]'s schema - similar enough that
    many files are drop-in compatible, but not identical.

This example shows the full pipeline: loading the model catalog, reading agent files, and running a named agent.

**1.** Create an agent file at `agents/coder.md`:

```markdown
---
name: coder
mode: all
description: A coding agent that can read, search, and edit files.
permission:
  read: allow
  write: allow
  edit: allow
  glob: allow
  grep: allow
  bash: allow
  webfetch: allow
  task: deny
---

You are a coding assistant. Use the available tools to complete the user's task.
```

**2.** Add the dependencies:

```toml
[dependencies]
llm-coding-tools-serdesai = "0.2"
llm-coding-tools-agents = "0.1"
llm-coding-tools-core = "0.2"
llm-coding-tools-models-dev = "0.1"
```

**3.** Run the agent:

```rust
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentBuildContext, AgentDefaults};
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load agent definitions from markdown files
    let mut catalog = AgentCatalog::new();
    AgentLoader::new().add_directory(&mut catalog, "./agents")?;

    // Sync the models.dev catalog (with ETag caching and offline fallback)
    let load_result = ModelsDevCatalog::load().await?;

    // Build runtime with a default model and the loaded agents
    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog)
        .defaults(AgentDefaults::with_model("synthetic/hf:MiniMaxAI/MiniMax-M2.5"))
        .build()?;

    // Create a shared build context (catalog + credentials)
    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(CredentialResolver::new()),
    );

    // Build a named agent and run it
    let agent = build_context.build("coder")?;
    let response = agent.run("Find all TODO comments in src/", ()).await?;
    println!("{}", response.output());
    Ok(())
}
```

!!! note "What just happened?"

    - **Agent markdown** defined the agent's name, permissions (default-deny),
      and system prompt in one file
    - **ModelsDevCatalog** fetched the latest model catalog from [models.dev]
      (or used a cached copy)
    - **AgentRuntimeBuilder** bundled the catalog with a default model
    - **AgentBuildContext** wired everything together with credentials
    - **`build("coder")`** resolved the agent by name, attached its permitted
      tools, and generated the system prompt

This example replicates the full [OpenCode]-style workflow - agent files,
model catalog, credentials.

For simpler use cases, you can instantiate tools directly without agents (see examples), or use `llm-coding-tools-core` with any LLM framework.

See [Embedding Guide](guides/embedding.md) for extra alternatives.

## Run the examples

The repository ships with complete, runnable examples:

```bash
# Basic agent setup
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access (restricted to allowed directories)
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# Sandboxed bash execution (Linux, requires bubblewrap)
cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai

# Agent catalog loading from markdown files
cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# Multi-agent task delegation (orchestrator delegates to sub-agents)
cargo run --example serdesai-task -p llm-coding-tools-serdesai
```

See [Examples](guides/examples.md) for the full list with descriptions and source links.

## Using blocking mode

All crates default to async via the `tokio` feature.

To use blocking mode, disable default features and enable `blocking`:

```toml
[dependencies]
llm-coding-tools-core = { version = "0.2", default-features = false, features = ["blocking"] }
```

## Next steps

- [Tools](tools.md) - every tool's behaviour, inputs, and outputs
- [Agents](agents.md) - define agents with markdown files and YAML frontmatter
- [Embedding Guide](guides/embedding.md) - integrate into your own application
- [Architecture](architecture.md) - understand how the 5 crates fit together

[SerdesAI]: https://crates.io/crates/serdes-ai
[OpenCode]: https://opencode.ai/
[models.dev]: https://models.dev
[bubblewrap]: https://github.com/containers/bubblewrap
