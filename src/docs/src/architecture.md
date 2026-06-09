# Architecture

reloaded-code is a Rust workspace with 5 crates that layer on top of each
other. This page explains how they connect and where your code plugs in.

## Crate dependency graph

```mermaid
graph TD
    core["reloaded-code-core<br/><i>Framework-agnostic tool primitives</i>"]
    agents["reloaded-code-agents<br/><i>Agent markdown loader + runtime</i>"]
    serdesai["reloaded-code-serdesai<br/><i>SerdesAI framework integration</i>"]
    bubblewrap["reloaded-code-bubblewrap<br/><i>Linux sandbox profiles</i>"]
    modelsdev["reloaded-code-models-dev<br/><i>models.dev catalog sync</i>"]
    providerconfig["reloaded-code-provider-config<br/><i>Provider config loading</i>"]

    agents --> core
    serdesai --> core
    serdesai --> agents
    serdesai -.->|optional| bubblewrap
    modelsdev --> core
    providerconfig --> core

    classDef default fill:#1C1C1C,stroke:#fa7774,strokeWidth:2px,color:#fff
```

## Layer overview

### reloaded-code-core

The foundation. Contains every tool implementation as a plain function
(`read_file`, `write_file`, `edit_file`, etc.), plus supporting types:

- **Path resolvers** - control which files tools can access
- **System prompt builder** - generates context-aware tool guidance
- **Permission engine** - last-match-wins rules with wildcard patterns
- **Custom tool registry + catalog** - framework-agnostic `ToolFactory`,
  `CustomToolRegistry`, and `ToolCatalogEntry` types
- **Credential resolver** - API key lookup with override support ([details](getting-started.md#credential-management))
- **Model catalog** - compact hash-table-based provider/model lookup
- **Hook types** - `HookSet`, `HookSetBuilder`, tool hook types
  (`ToolHook`, `ToolOriginal`, `ToolHookFuture`, `ToolExecutor`,
  `ToolCallContext`, `ToolRequest`), and hook run event types
  (`HookRunContext`, `EndReason`). See [Hooks](hooks).

Core is **framework-agnostic**: it has no dependencies on any specific LLM
framework. Your integration layer wraps these functions into framework-specific
tool types.

### reloaded-code-agents

Loads agent definitions from markdown files with YAML frontmatter. Provides:

- **AgentLoader** - scans directories for `.md` agent files
- **AgentCatalog** - name-to-config lookup table
- **AgentRuntime** - bundles catalog + defaults + permissions + task settings
  + the registered `HookSet`
- **AgentRuntimeBuilder** - accepts core tool catalogs, custom tool factories,
  and a `HookSet` (`.hooks(set)`)

The agent file format mirrors [OpenCode]'s schema - similar enough that many
files are drop-in compatible, but [not identical](migration.md). The most
notable difference is **default-deny** permissions: tools must be explicitly
allowed.

### reloaded-code-serdesai

The ready-to-use integration layer for the [SerdesAI] framework. It:

- Wraps core tool functions into [SerdesAI] `Tool` trait implementations
- Bridges 15 provider types to concrete [SerdesAI] model constructors
- Builds runnable `Agent` instances from agent markdown definitions
- Handles multi-agent task delegation with depth limits

If you use [SerdesAI], this is the only crate you need. If you use a different
framework, build your own adapters using core.

### reloaded-code-bubblewrap

Linux-only. Builds and manages [bubblewrap] sandbox profiles for shell command
isolation. Two presets:

- **Public Bot** - for untrusted input (no network, minimal filesystem)
- **Trusted Maintenance** - for trusted automation (read-only host `/`, network on)

### reloaded-code-models-dev

Syncs the online [models.dev](https://models.dev) catalog into a compact
`ModelCatalog`. Features:

- ETag-based conditional HTTP requests
- [zstd]-compressed cache (~24 KiB for ~3000 models)
- Offline fallback when network is unavailable
- Cache load in ~0.3 ms

### reloaded-code-provider-config

Loads provider override configuration and turns it into provider catalog entries
that can be merged with or replace the defaults from models.dev.

## Where your code plugs in

There are two integration paths:

**Path A: Use [SerdesAI]** - Add `reloaded-code-serdesai`, attach tools to an
`AgentBuilder`, and run. This is the fastest path to a working agent.

**Path B: Bring your own framework** - Depend on `reloaded-code-core`,
implement your framework's tool trait by calling the core functions, and use
`SystemPromptBuilder` to generate the system prompt. See
[Custom Framework](guides/custom-framework.md) for a walkthrough.

```mermaid
graph LR
    subgraph "Path A: SerdesAI"
        A1["[SerdesAI] crate"] --> A2[Runnable Agent]
    end
    subgraph "Path B: Custom"
        B1[core crate] --> B2[Your Tool Adapter] --> B3[Any Framework]
    end
```

[SerdesAI]: https://crates.io/crates/serdes-ai
[OpenCode]: https://opencode.ai/
[bubblewrap]: https://github.com/containers/bubblewrap
[zstd]: https://facebook.github.io/zstd/
