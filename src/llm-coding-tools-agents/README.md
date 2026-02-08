# llm-coding-tools-agents

Load OpenCode agent markdown files into a typed Rust catalogue.

This crate is a loader for the [OpenCode agent schema](https://opencode.ai/docs/agents/).

It is a drop-in replacement for OpenCode agent files: agents you create for OpenCode should load here unchanged.

## What it provides

- [`AgentLoader`] for loading agent configs from directories, files, or in-memory markdown.
- [`AgentCatalog`] for storing and looking up loaded [`AgentConfig`] entries.
- [`RulesetExt`] for converting frontmatter `permission` data into runtime [`Ruleset`]s.

## Quick start

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoadError, AgentLoader};
use std::path::Path;

let loader = AgentLoader::new();
let mut catalog = AgentCatalog::new();

loader.add_directory(
    &mut catalog,
    "/home/user/.opencode",
    None::<fn(&Path, &AgentLoadError)>,
)?;

for agent in catalog.iter() {
    println!("{}: {}", agent.name, agent.description);
}
# Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
```

## Agent file format

```markdown
---
mode: subagent
description: Reads and summarises files
model: synthetic/hf:moonshotai/Kimi-K2.5
permission:
  read: allow
  task: deny
---

Prompt body here...
```

For field behaviour, see OpenCode docs for [`mode`](https://opencode.ai/docs/agents#mode), [`model`](https://opencode.ai/docs/agents#model), and [`permissions`](https://opencode.ai/docs/agents#permissions).

## Compatibility notes

This library does not provide interactive UX extensions (for example, TUI approval flows).
To avoid false expectations, settings that require interaction are rejected, while settings with no runtime effect are accepted and ignored:

- [`permission.task`](https://opencode.ai/docs/agents#task-permissions): `ask` is rejected with a schema validation error (`allow`/`deny` only), because `ask` is an interactive approval mode in OpenCode ([docs](https://opencode.ai/docs/permissions#what-ask-does)).
- [`hidden`](https://opencode.ai/docs/agents#hidden) is accepted for compatibility, but ignored at runtime.

## Integration

This crate only loads and validates agent configs.
Pass [`AgentCatalog`] to your runtime adapter (for example, `llm-coding-tools-serdesai`) to build registries and Task tooling.

[`Ruleset`]: llm_coding_tools_core::permissions::Ruleset
