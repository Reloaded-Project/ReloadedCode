# llm-coding-tools

[![CI](https://github.com/Sewer56/llm-coding-tools/actions/workflows/rust.yml/badge.svg)](https://github.com/Sewer56/llm-coding-tools/actions)

Lightweight, heavily optimized coding tool implementations for LLM-powered
development agents.

Suitable for server use (<3 MiB), or as building blocks for your own TUI coding agent.

## About This Workspace

This workspace contains multiple Rust crates for integrating coding tools with
LLM agents:

- **[llm-coding-tools-core](./src/llm-coding-tools-core/)**:
  Framework-agnostic core operations and utilities
- **[llm-coding-tools-agents](./src/llm-coding-tools-agents/)**:
  OpenCode agent markdown loader and typed catalogue
- **[llm-coding-tools-serdesai](./src/llm-coding-tools-serdesai/)**:
  serdesAI framework-specific Tool implementations
- **[llm-coding-tools-bubblewrap](./src/llm-coding-tools-bubblewrap/)**:
  Sandboxing for Bash tool on Linux based on `bwrap` tool
- **[llm-coding-tools-models-dev](./src/llm-coding-tools-models-dev/)**:
  models.dev catalog sync with cached fallback and ETag refresh

## Features

- **File Operations**: Read, write, edit files with line-numbered output
- **Search**: Glob pattern matching and regex content search  
- **Shell**: Cross-platform command execution with timeout; with optional sandboxing on Linux.
- **Web**: URL fetching with HTML-to-markdown conversion
- **Path Security**: Choose between unrestricted or sandboxed file access
- **OpenCode Agents**: Support for OpenCode-style agents
- **Model Catalog Sync**: Download and cache the models.dev catalog for
  provider/model lookups
- **Optimized System Prompt**: Built-in tool guidance stays compact.
    Full tool set is ~2000 tokens; search-only drops to ~560

## Feature Flags (llm-coding-tools-core)

- `tokio` (default): Async mode with tokio runtime
- `blocking`: Sync/blocking mode, mutually exclusive with `async`
- `linux-bubblewrap`: Sandboxing for `bash` tool via Linux `bwrap` tool

## Quick Start

Pick the crate that matches your use case:

```toml
[dependencies]
llm-coding-tools-core = "0.2"         # Framework-agnostic tool implementations
llm-coding-tools-agents = "0.1"       # OpenCode agent markdown loader
llm-coding-tools-models-dev = "0.1"   # models.dev catalog sync and cache
llm-coding-tools-serdesai = "0.2"     # serdesAI integration
llm-coding-tools-bubblewrap = "0.1"   # Linux Bubblewrap profile and wrapper helpers
```

For a runnable agent setup, start with `llm-coding-tools-serdesai` and the
examples below.

## Examples

```bash
# serdesAI framework - Basic agent setup
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# serdesAI framework - Sandboxed file access
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# serdesAI framework - Agent catalog loading
cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# serdesAI framework - Task delegation
cargo run --example serdesai-task -p llm-coding-tools-serdesai
```

## Documentation

- [llm-coding-tools-core README](./src/llm-coding-tools-core/README.md)
- [llm-coding-tools-agents README](./src/llm-coding-tools-agents/README.md)
- [llm-coding-tools-serdesai README](./src/llm-coding-tools-serdesai/README.md)
- [llm-coding-tools-bubblewrap README](./src/llm-coding-tools-bubblewrap/README.md)
- [llm-coding-tools-models-dev README](./src/llm-coding-tools-models-dev/README.md)
- [Sandbox profiles and operator checklist](./SANDBOX-PROFILES.md)
- [Developer Guidelines](./src/AGENTS.md)

## Contributing

Contributions are welcome! Please ensure all tests pass and the code follows
our guidelines.

## Deprecation Notice

**Rig framework support (`llm-coding-tools-rig`) has been removed**
(commit 17158db) due to library bugs that prevented examples from running
reliably.

You're welcome to submit a PR re-adding rig support if you're willing to
maintain it. Since I don't use rig personally, I'm not able to actively
maintain that integration. Alternatively, you can create your own crate
building on `llm-coding-tools-core` directly.

## License

Licensed under [Apache 2.0](./LICENSE).
