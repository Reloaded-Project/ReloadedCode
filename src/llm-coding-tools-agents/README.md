# llm-coding-tools-agents

Load OpenCode agent markdown files into Rust.

This crate reads agent definitions from markdown files with YAML frontmatter,
following the [OpenCode agent schema](https://opencode.ai/docs/agents/).

Agents you create for OpenCode work here unchanged.

## Loading agents

Use [`AgentLoader`] to read agent files from a directory, then store them in
an [`AgentCatalog`] for lookup by name:

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader};

let loader = AgentLoader::new();
let mut catalog = AgentCatalog::new();

loader.add_directory(&mut catalog, "/home/user/.opencode")?;

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

For field behaviour, see OpenCode docs for
[`mode`](https://opencode.ai/docs/agents#mode),
[`model`](https://opencode.ai/docs/agents#model), and
[`permissions`](https://opencode.ai/docs/agents#permissions).

## Building agents

Framework adapters (like `llm-coding-tools-serdesai`) use [`AgentRuntime`] to
build runnable agents. An `AgentRuntime` bundles your loaded agents with default
settings and available tools:

```rust,no_run
use llm_coding_tools_agents::{
    AgentCatalog, AgentDefaults, AgentLoader, AgentRuntimeBuilder,
};

let loader = AgentLoader::new();
let mut catalog = AgentCatalog::new();
loader.add_directory(&mut catalog, "/home/user/.opencode")?;

let runtime = AgentRuntimeBuilder::new()
    .catalog(catalog)
    .defaults(AgentDefaults::with_model("openai/gpt-4o-mini"))
    // .max_task_depth(5)  // optional; defaults to 3 Task hops
    // .tools(my_custom_tools)  // optional; defaults to read/write/edit/glob/grep/bash/webfetch/todoread/todowrite/task
    .build();

// Pass `runtime` to your framework adapter to build agents by name
# Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
```

## Compatibility notes

This library does not provide interactive UX extensions (for example, TUI
approval flows).

To avoid false expectations, settings that require interaction are rejected,
while settings with no runtime effect are accepted and ignored:

- Unspecified permissions default to `deny` for normal tools. `permission.task`
  is special: if omitted, Task still allows delegation to callable
  `mode: all` / `mode: subagent` targets for OpenCode compatibility.
- [`permission.task`](https://opencode.ai/docs/agents#task-permissions):
  `ask` is rejected with a schema validation error (`allow`/`deny` only),
  because `ask` is an interactive approval mode in OpenCode
  ([docs](https://opencode.ai/docs/permissions#what-ask-does)).
- [`hidden`](https://opencode.ai/docs/agents#hidden) is accepted for
  compatibility, but ignored at runtime.
