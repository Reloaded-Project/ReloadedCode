# llm-coding-tools-agents

Agent configuration loading from OpenCode-style markdown files with YAML frontmatter.

## Features

- Parse markdown files with YAML frontmatter
- Preprocess frontmatter to handle inline colons (e.g., `model: openai:provider/model-id[:tag]`)
- Scan directories for agent configs matching `agent/**/*.md` and `agents/**/*.md`
- Derive agent names from file paths
- Permission evaluation with wildcard pattern matching (last-match-wins)

## Usage

Load agent configurations into [`AgentCatalog`] using [`AgentLoader`]:

```rust,no_run
use llm_coding_tools_agents::{AgentLoader, AgentCatalog};
use std::path::Path;

let mut loader = AgentLoader::new();
let mut catalog = AgentCatalog::new();
let opencode_dir = std::path::PathBuf::from("/home/user/.opencode");
loader.add_directory(&mut catalog, &opencode_dir)?;

for config in catalog.iter() {
    println!("{}: {}", config.name, config.description);
}
# Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
```

## Agent File Format

```markdown
---
mode: subagent
description: Explores codebase structure
model: openai:provider/model-id[:tag]
permission:
  read: allow
  task: deny
---

Prompt body goes here...
```

**Note**: Provider selection is driven by the `provider:` prefix, not by URL inspection. OpenAI-compatible endpoints should still use `openai:` with a custom base URL provided via provider overrides.


### Mode Options

The `mode` field controls how the agent can be invoked:

- `subagent`: Runs as a supportive agent invoked by a primary agent. Can execute tasks but cannot spawn other subagents.
- `primary`: The main agent that can spawn or coordinate subagents. Full tool access including Task tool for invoking other agents.
- `all`: Agent can run as primary or subagent.
- If `mode` is omitted, loader defaults to `all`.

## Task Tool (Registry-Driven Flow)

The Task tool allows agents to invoke other agents with permission-based access control.
Task types ([`TaskInput`] and [`TaskOutput`]) are provided by `llm-coding-tools-core`.
The Task tool behavior is implemented in framework adapters (serdesAI).

### Registry-Driven Task Flow

The flow for using Task tools is:

1. **Load agent configs** into [`AgentCatalog`] using [`AgentLoader`]
2. **Build a framework registry** using `AgentRegistryBuilder` (serdesAI)
3. **Construct `TaskTool`** from the registry and caller permission rules

#### Example for serdesAI:

See `examples/serdesai-agents.rs` for the complete example.

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, Ruleset, Rule, PermissionAction};
use llm_coding_tools_core::operations::{TaskInput, TaskOutput};
use llm_coding_tools_serdesai::{AgentDefaults, AgentRegistryBuilder, ProviderOverrides, TaskTool, default_tools, TodoState};
use std::sync::Arc;

// 1) Load agent configs
let mut catalog = AgentCatalog::new();
AgentLoader::new().add_directory(&mut catalog, "/home/user/.opencode")?;

// 2) Build framework registry
let defaults = AgentDefaults {
    model: "openai:hf:zai-org/GLM-4.7".into(),
    model_resolver: None,
    provider_overrides: ProviderOverrides::new(),
    api_key: Some(std::env::var("OPENAI_API_KEY").unwrap_or_default()),
    base_url: Some("https://api.synthetic.new/openai/v1".into()),
    temperature: None,
    top_p: None,
    options: Default::default(),
};
let tools = default_tools(true, None, TodoState::new());
let registry = AgentRegistryBuilder::<()>::new(defaults, tools).build(&catalog)?;

// 3) Create Task tool
let mut rules = Ruleset::new();
rules.push(Rule::new("task", "*", PermissionAction::Allow));
let deps = ();
let task_tool = TaskTool::new(Arc::new(registry), rules, Arc::new(deps));
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Permission Enforcement

The framework `TaskTool` implementations enforce access validation:

1. Checks if the agent exists (returns validation error if not)
2. Verifies the agent is invocable (`subagent` or `all`)
3. Checks caller's `task` permission for the requested agent
4. Uses the agent's permission rules to filter available tools
5. `permission.task` supports only `allow`/`deny`; `ask` is rejected during validation.

Framework registries precompute allowed tools based on each agent's permission rules
during registry construction.

## Migration from Legacy APIs

The legacy `TaskRunner`, `TaskToolCore`, and `SubagentRegistry` types have been removed.
Migrate to the new flow as follows:

| Legacy API         | New API                                     |
| ------------------ | ------------------------------------------- |
| `SubagentRegistry` | `AgentCatalog` + framework `AgentRegistry`  |
| `TaskRunner`       | Not needed - use registry-driven `TaskTool` |
| `TaskToolCore`     | Not needed - use framework `TaskTool` types |
| `TaskError`        | Framework-specific error types              |

For complete migration examples, see:
- `examples/serdesai-agents.rs` (PROMPT-06)

## Permission System

Permissions use a ruleset with allow/deny actions and wildcard patterns.
Evaluation follows a last-match-wins policy with default deny.

```rust
use llm_coding_tools_agents::{Ruleset, Rule, PermissionAction};

let mut ruleset = Ruleset::new();
ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
ruleset.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));

assert!(ruleset.is_allowed("task", "orchestrator-builder"));
assert!(!ruleset.is_allowed("task", "random-agent"));
```
