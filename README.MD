<div align="center">

# ReloadedCode

**Production-grade coding agent tools in Rust. <abbr title="~10 MiB PSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · RSS ~13 MiB.">~10 MiB</abbr>. No TUI. Embed it anywhere.**

[![CI](https://github.com/Reloaded-Project/ReloadedCode/actions/workflows/rust.yml/badge.svg)](https://github.com/Reloaded-Project/ReloadedCode/actions) [![crates.io](https://img.shields.io/crates/v/reloaded-code-core.svg)](https://crates.io/crates/reloaded-code-core) [![docs.rs](https://img.shields.io/docsrs/reloaded-code-core)](https://docs.rs/reloaded-code-core) [![License](https://img.shields.io/crates/l/reloaded-code-core)](./LICENSE)

[Get Started](#quick-start) · [Documentation](https://reloaded-project.github.io/ReloadedCode/) · [API Reference](https://docs.rs/reloaded-code-core) · [Examples](#examples)

</div>

---

## Why this project?

ReloadedCode started as "an OpenCode for servers." Headless,
sandboxed, and cheap to host for non-commercial use.

[OpenCode](https://opencode.ai) is a great interactive coding agent, but it's a
<abbr title="opencode v1.14.21&#10;serve: 305 MiB RSS&#10;TUI: 525 MiB RSS&#10;&#10;v1.4.2&#10;serve: 392 MiB RSS&#10;TUI: 679 MiB RSS">~305 MiB</abbr> TypeScript application that runs as a separate process.
What if you need those same tools for a **server**? A **Discord bot**?
A **CI pipeline**? A **custom product**?

**ReloadedCode** ships the same agent tools as a Rust library.
Shell sandboxing. Default-deny permissions. ~10 MiB footprint.

|              | OpenCode                                                                                                                                                | ReloadedCode                                                                                                                                                                                                                                                                                             |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Language     | TypeScript                                                                                                                                              | Rust                                                                                                                                                                                                                                                                                                     |
| Memory       | <abbr title="opencode v1.14.21&#10;serve: 305 MiB RSS&#10;TUI: 525 MiB RSS&#10;&#10;v1.4.2&#10;serve: 392 MiB RSS&#10;TUI: 679 MiB RSS">~305 MiB</abbr> | <abbr title="~13 MiB RSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · PSS ~10 MiB.">~13 MiB</abbr> |
| Interface    | TUI / Desktop / IDE                                                                                                                                     | Library (headless)                                                                                                                                                                                                                                                                                       |
| Agent format | Markdown + YAML                                                                                                                                         | Similar format                                                                                                                                                                                                                                                                                           |
| Embeddable   | HTTP API                                                                                                                                                | Rust crate                                                                                                                                                                                                                                                                                               |

## Features

- **10 Built-in tools** - read, write, edit, glob, grep, bash, webfetch, todoread, todowrite, task
- **Agents similar to [OpenCode](https://opencode.ai)** - load agent markdown files with YAML frontmatter
- **Multi-agent delegation** - orchestrator pattern with depth-limited task chains
- **Linux sandboxing** - bubblewrap profiles for shell isolation (Public Bot + Trusted Maintenance)
- **Path security** - restrict file access with allowed directories and glob-based rules
- **Model catalog** - sync [models.dev](https://models.dev) with ETag caching (~3000 models, ~24 KiB cache)
- **Permissions** - default-deny with last-match-wins rules and wildcard patterns
- **Optimized system prompt** - auto-generated, ~2000 tokens full / ~560 search-only
- **Async + Sync** - every tool compiles as tokio async or blocking. Zero overhead.
- **Framework-agnostic core** - use the serdesAI integration or bring your own framework
- **15 LLM providers** - OpenAI, Anthropic, Google, Groq, Mistral, Ollama, Azure, Bedrock, and more
- **Semver-guaranteed API** - 6-platform CI matrix, 11 semver surfaces, clippy -D warnings

## Quick Start

```toml
[dependencies]
reloaded-code-serdesai = "0.2"
```

**1.** Create an agent file (markdown + YAML frontmatter similar to [OpenCode](https://opencode.ai)):

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

**2.** Load the catalog, build the agent, and run:

```rust
use reloaded_code_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use reloaded_code_core::CredentialResolver;
use reloaded_code_models_dev::ModelsDevCatalog;
use reloaded_code_serdesai::{AgentBuildContext, AgentDefaults};
use std::{path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load agents from the "agents" directory.
    let mut catalog = AgentCatalog::new();
    AgentLoader::new().add_directory(&mut catalog, "./agents")?;

    // Supports any model from https://models.dev
    let load_result = ModelsDevCatalog::load().await?;

    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog) // Default model if not specified by agent.
        .defaults(AgentDefaults::with_model("synthetic/hf:MiniMaxAI/MiniMax-M2.5"))
        .build()?;

    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(CredentialResolver::new()),
    );

    let agent = build_context.build("coder")?;
    let response = agent.run("Find all TODO comments in src/", ()).await?;
    println!("{}", response.output());
    Ok(())
}
```

## Crate Map

| Crate                                                                     | Version | Description                                                                                      |
| ------------------------------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------ |
| [**reloaded-code-core**](./src/reloaded-code-core/)                       | 0.2     | Framework-agnostic tool implementations, path resolvers, permissions, custom tool registry       |
| [**reloaded-code-agents**](./src/reloaded-code-agents/)                   | 0.1     | agent markdown loader similar to [OpenCode](https://opencode.ai), typed catalog, runtime builder |
| [**reloaded-code-serdesai**](./src/reloaded-code-serdesai/)               | 0.2     | SerdesAI framework integration, tool adapters, 15 provider bridges, task delegation              |
| [**reloaded-code-bubblewrap**](./src/reloaded-code-bubblewrap/)           | 0.1     | Linux bubblewrap sandbox profiles (Public Bot + Trusted Maintenance)                             |
| [**reloaded-code-models-dev**](./src/reloaded-code-models-dev/)           | 0.1     | models.dev catalog sync with ETag caching and offline fallback                                   |
| [**reloaded-code-provider-config**](./src/reloaded-code-provider-config/) | 0.1     | Provider configuration loading and provider catalog overrides                                    |

## Examples

```bash
# Basic agent setup with all tools
cargo run --example serdesai-basic -p reloaded-code-serdesai

# Sandboxed file access (restricted to allowed directories)
cargo run --example serdesai-sandboxed -p reloaded-code-serdesai

# Sandboxed bash execution (Linux, requires bubblewrap)
cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p reloaded-code-serdesai

# Agent catalog loading from markdown files
cargo run --example serdesai-agents -p reloaded-code-serdesai

# Multi-agent task delegation (orchestrator + reader sub-agent)
cargo run --example serdesai-task -p reloaded-code-serdesai
```

## Documentation

- **[Documentation Site](https://reloaded-project.github.io/ReloadedCode/)** - guides, architecture, examples
- [reloaded-code-core README](./src/reloaded-code-core/README.md)
- [reloaded-code-agents README](./src/reloaded-code-agents/README.md)
- [reloaded-code-serdesai README](./src/reloaded-code-serdesai/README.md)
- [reloaded-code-bubblewrap README](./src/reloaded-code-bubblewrap/README.md)
- [reloaded-code-models-dev README](./src/reloaded-code-models-dev/README.md)
- [Sandbox profiles and operator checklist](https://reloaded-project.github.io/ReloadedCode/extra_sandboxing_notes/)
- [API Reference (docs.rs)](https://docs.rs/reloaded-code-core)

## Contributing

Contributions are welcome! Please ensure all tests pass and the code follows
our guidelines.

## License

Licensed under [Apache 2.0](./LICENSE).
