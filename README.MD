<div align="center">

# llm-coding-tools

**Production-grade coding agent tools in Rust. <abbr title="~10 MiB PSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · RSS ~13 MiB.">~10 MiB</abbr>. No TUI. Embed it anywhere.**

[![CI](https://github.com/Sewer56/llm-coding-tools/actions/workflows/rust.yml/badge.svg)](https://github.com/Sewer56/llm-coding-tools/actions) [![crates.io](https://img.shields.io/crates/v/llm-coding-tools-core.svg)](https://crates.io/crates/llm-coding-tools-core) [![docs.rs](https://img.shields.io/docsrs/llm-coding-tools-core)](https://docs.rs/llm-coding-tools-core) [![License](https://img.shields.io/crates/l/llm-coding-tools-core)](./LICENSE)

[Get Started](#quick-start) · [Documentation](https://sewer56.github.io/llm-coding-tools/) · [API Reference](https://docs.rs/llm-coding-tools-core) · [Examples](#examples)

</div>

---

## Why this project?

[OpenCode](https://opencode.ai) is a fun, fast-moving TUI coding agent - but it ships
breaking changes regularly. It's a TypeScript application that uses <abbr title="opencode v1.4.2&#10;  • serve: 392 MiB RSS&#10;  • TUI: 679 MiB RSS">~400 MiB</abbr> of RAM
and runs as a separate process.

What if you need the same agent tooling for a **server**? A **Discord bot**?
A **CI pipeline**? A **custom product**?

**llm-coding-tools** takes the core agent tooling and ships it as a headless
Rust library. Same tool set, same agent format, same system prompts - at a
fraction of the resource cost.

|              | OpenCode                                                                                          | llm-coding-tools                                                                                                                                                                                                                                                                                         |
| ------------ | ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Language     | TypeScript                                                                                        | Rust                                                                                                                                                                                                                                                                                                     |
| Memory       | <abbr title="opencode v1.4.2&#10;  • serve: 392 MiB RSS&#10;  • TUI: 679 MiB RSS">~400 MiB</abbr> | <abbr title="~10 MiB PSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · RSS ~13 MiB.">~10 MiB</abbr> |
| Interface    | TUI / Desktop / IDE                                                                               | Library (headless)                                                                                                                                                                                                                                                                                       |
| Agent format | Markdown + YAML                                                                                   | Similar format                                                                                                                                                                                                                                                                                           |
| Embeddable   | HTTP API                                                                                          | Rust crate                                                                                                                                                                                                                                                                                               |

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
llm-coding-tools-serdesai = "0.2"
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
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentBuildContext, AgentDefaults};
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

| Crate                                                                 | Version | Description                                                                                      |
| --------------------------------------------------------------------- | ------- | ------------------------------------------------------------------------------------------------ |
| [**llm-coding-tools-core**](./src/llm-coding-tools-core/)             | 0.2     | Framework-agnostic tool implementations, path resolvers, permissions, system prompt builder      |
| [**llm-coding-tools-agents**](./src/llm-coding-tools-agents/)         | 0.1     | agent markdown loader similar to [OpenCode](https://opencode.ai), typed catalog, runtime builder |
| [**llm-coding-tools-serdesai**](./src/llm-coding-tools-serdesai/)     | 0.2     | SerdesAI framework integration, tool adapters, 15 provider bridges, task delegation              |
| [**llm-coding-tools-bubblewrap**](./src/llm-coding-tools-bubblewrap/) | 0.1     | Linux bubblewrap sandbox profiles (Public Bot + Trusted Maintenance)                             |
| [**llm-coding-tools-models-dev**](./src/llm-coding-tools-models-dev/) | 0.1     | models.dev catalog sync with ETag caching and offline fallback                                   |

## Examples

```bash
# Basic agent setup with all tools
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access (restricted to allowed directories)
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# Sandboxed bash execution (Linux, requires bubblewrap)
cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai

# Agent catalog loading from markdown files
cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# Multi-agent task delegation (orchestrator + reader sub-agent)
cargo run --example serdesai-task -p llm-coding-tools-serdesai
```

## Documentation

- **[Documentation Site](https://sewer56.github.io/llm-coding-tools/)** - guides, architecture, examples
- [llm-coding-tools-core README](./src/llm-coding-tools-core/README.md)
- [llm-coding-tools-agents README](./src/llm-coding-tools-agents/README.md)
- [llm-coding-tools-serdesai README](./src/llm-coding-tools-serdesai/README.md)
- [llm-coding-tools-bubblewrap README](./src/llm-coding-tools-bubblewrap/README.md)
- [llm-coding-tools-models-dev README](./src/llm-coding-tools-models-dev/README.md)
- [Sandbox profiles and operator checklist](./SANDBOX-PROFILES.md)
- [API Reference (docs.rs)](https://docs.rs/llm-coding-tools-core)

## Contributing

Contributions are welcome! Please ensure all tests pass and the code follows
our guidelines.

## License

Licensed under [Apache 2.0](./LICENSE).
