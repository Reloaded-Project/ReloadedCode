---
hide:
  - toc
---

<link rel="stylesheet" href="assets/landing.css">

<div class="landing-hero">
  <h1>llm-coding-tools</h1>
  <p class="tagline">
    Production-grade coding agent tools in Rust.<br>
    <abbr title="~10 MiB PSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · RSS ~13 MiB.">~10 MiB</abbr>. No TUI. Embed it anywhere.
  </p>
</div>

<div class="landing-badges">
  <img alt="CI" src="https://github.com/Sewer56/llm-coding-tools/actions/workflows/rust.yml/badge.svg">
  <img alt="crates.io" src="https://img.shields.io/crates/v/llm-coding-tools-core.svg">
  <img alt="docs.rs" src="https://img.shields.io/docsrs/llm-coding-tools-core">
  <img alt="License" src="https://img.shields.io/crates/l/llm-coding-tools-core">

</div>

<div class="landing-cta">
  <a href="getting-started" class="md-button md-button--primary">Get Started</a>
  <a href="https://github.com/Sewer56/llm-coding-tools" class="md-button">View on GitHub</a>
  <a href="https://docs.rs/llm-coding-tools-core/latest/llm_coding_tools_core/" class="md-button">API Reference</a>
  <a href="guides/examples/" class="md-button">Examples</a>
</div>

---

## Why this project?

[OpenCode] is a fun, fast-moving TUI coding agent - but it ships breaking changes
regularly. It's a **TypeScript application** that uses <abbr title="opencode v1.4.2&#10;  • serve: 392 MiB RSS&#10;  • TUI: 679 MiB RSS">~400 MiB</abbr> of RAM, and it runs
as a separate process. What if you need the same agent tooling for a server? A Discord bot?
A CI pipeline? Custom software?

**llm-coding-tools** takes the core agent tooling and ships it as a **lightweight headless Rust
library** - same tool set, similar agent format, same system prompts - at a fraction
of the resource cost.

<div class="landing-stats">
  <div class="stat-card">
    <div class="stat-value"><abbr title="~10 MiB PSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · RSS ~13 MiB.">~10 MiB</abbr></div>
    <div class="stat-label">Memory usage</div>
  </div>
  <div class="stat-card">
    <div class="stat-value">10</div>
    <div class="stat-label">Built-in tools</div>
  </div>
  <div class="stat-card">
    <div class="stat-value">~2K</div>
    <div class="stat-label">System prompt tokens</div>
  </div>
  <div class="stat-card">
    <div class="stat-value">6 / 11</div>
    <div class="stat-label">CI platforms / semver surfaces</div>
  </div>
</div>

## Features

<div class="feature-grid">
  <div class="feature-card">
    <h3>📄 File Operations</h3>
    <p>Read, write, and edit files with line-numbered output, offset/limit, and exact text replacement.</p>
  </div>
  <div class="feature-card">
    <h3>🔍 Search</h3>
    <p>Glob pattern matching and regex content search with match metadata and configurable limits.</p>
  </div>
  <div class="feature-card">
    <h3>💻 Shell Execution</h3>
    <p>Cross-platform command execution with timeout, captured output, and optional Linux sandboxing.</p>
  </div>
  <div class="feature-card">
    <h3>🌐 Web Fetch</h3>
    <p>Fetch URLs and convert HTML to markdown. Configurable timeouts and size limits.</p>
  </div>
  <div class="feature-card">
    <h3>🔒 Sandboxing</h3>
    <p>Linux <a href="https://github.com/containers/bubblewrap">bubblewrap</a> profiles for shell isolation. Network isolation, filtered filesystem, scrubbed env.</p>
  </div>
  <div class="feature-card">
    <h3>🤖 Agent Runtime</h3>
    <p>Load agent markdown files based on [OpenCode]'s schema. Multi-agent delegation with depth guards.</p>
  </div>
  <div class="feature-card">
    <h3>🗄️ Model Catalog</h3>
    <p>Sync the <a href="https://models.dev">models.dev</a> catalog with ETag caching, zstd compression, and offline fallback.</p>
  </div>
  <div class="feature-card">
    <h3>🔑 Permissions</h3>
    <p>Default-deny tool access with last-match-wins rules. Wildcard patterns for delegation control.</p>
  </div>
  <div class="feature-card">
    <h3>⚡ Async + Sync</h3>
    <p>Every tool compiles as async (<a href="https://tokio.rs">tokio</a>) or blocking. Zero overhead at the call site.</p>
  </div>
  <div class="feature-card">
    <h3>🧩 Embeddable</h3>
    <p>Framework-agnostic core. Use the <a href="https://crates.io/crates/serdes-ai">serdesAI</a> integration or build your own with the core primitives.</p>
  </div>
</div>

## Quick Start

**1.** Create an agent file (`agents/coder.md`):

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

## The Crate Map

<div class="crate-grid">
  <div class="crate-card">
    <h3><a href="https://crates.io/crates/llm-coding-tools-core">core</a></h3>
    <p>Framework-agnostic tools for building coding agents. File operations, search, shell, permissions, system prompts - use with any LLM framework.</p>
  </div>
  <div class="crate-card">
    <h3><a href="https://crates.io/crates/llm-coding-tools-agents">agents</a></h3>
    <p>Load agent markdown files based on [OpenCode]'s schema into a typed catalog. Default-deny permissions with granular path matching.</p>
  </div>
  <div class="crate-card">
    <h3><a href="https://crates.io/crates/llm-coding-tools-serdesai">serdesai</a></h3>
    <p>Ready-to-use <a href="https://crates.io/crates/serdes-ai">SerdesAI</a> integration. 15 provider bridges, multi-agent task delegation with depth guards.</p>
  </div>
  <div class="crate-card">
    <h3><a href="https://crates.io/crates/llm-coding-tools-bubblewrap">bubblewrap</a></h3>
    <p>Sandbox shell execution on Linux. Network-isolated, filesystem-filtered profiles for untrusted input. Two presets included.</p>
  </div>
  <div class="crate-card">
    <h3><a href="https://crates.io/crates/llm-coding-tools-models-dev">models-dev</a></h3>
    <p>Sync the <a href="https://models.dev">models.dev</a> catalog. ETag caching, offline fallback. ~3000 models in ~24 KiB.</p>
  </div>
</div>

## Comparison with OpenCode

<table class="comparison-table">
  <thead>
    <tr>
      <th>Aspect</th>
      <th>OpenCode</th>
      <th>llm-coding-tools</th>
    </tr>
  </thead>
  <tbody>
    <tr><td>Language</td><td>TypeScript</td><td>Rust</td></tr>
    <tr><td>Runtime</td><td>Bun</td><td><a href="https://tokio.rs">tokio</a> / blocking</td></tr>
    <tr><td>Memory</td><td><abbr title="opencode v1.4.2&#10;  • serve: 392 MiB RSS&#10;  • TUI: 679 MiB RSS">~400 MiB</abbr></td><td><abbr title="~10 MiB PSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · RSS ~13 MiB.">~10 MiB</abbr></td></tr>
    <tr><td>Interface</td><td>TUI / Desktop / IDE</td><td>Library (headless)</td></tr>
    <tr><td>Agent format</td><td>Markdown + YAML</td><td>Similar format</td></tr>
    <tr><td>Permissions</td><td>Default-allow + ask</td><td>Default-deny</td></tr>
    <tr><td>Tool set</td><td>14 tools</td><td>10 tools (core set)</td></tr>
    <tr><td>LLM framework</td><td>AI SDK (TypeScript)</td><td><a href="https://crates.io/crates/serdes-ai">SerdesAI</a> / bring your own</td></tr>
    <tr><td>Sandboxing</td><td>-</td><td>Linux <a href="https://github.com/containers/bubblewrap">bubblewrap</a> profiles</td></tr>
    <tr><td>Embeddable</td><td>Client/server API</td><td>Rust library (crate)</td></tr>
  </tbody>
</table>

See [Comparison with OpenCode](comparison.md) for a deeper breakdown.

[OpenCode]: https://opencode.ai/
[SerdesAI]: https://crates.io/crates/serdes-ai