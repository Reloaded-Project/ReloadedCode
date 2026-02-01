# llm-coding-tools-agents

Agent configuration loading from OpenCode-style markdown files with YAML frontmatter.

## Features

- Parse markdown files with YAML frontmatter
- Preprocess frontmatter to handle inline colons (e.g., `model: provider/model:tag`)
- Scan directories for agent configs matching `agent/**/*.md` and `agents/**/*.md`
- Derive agent names from file paths
- Permission evaluation with wildcard pattern matching (last-match-wins)

## Usage

Load agent configurations into [`AgentCatalog`] using [`AgentLoader`]:

```rust
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
model: openrouter:provider/model-id[:tag]
permission:
  read: allow
  task: deny
---

Prompt body goes here...
```

### Mode Options

The `mode` field controls how the agent can be invoked:

- `subagent`: Runs as a supportive agent invoked by a primary agent. Can execute tasks but cannot spawn other subagents.
- `primary`: The main agent that can spawn or coordinate subagents. Full tool access including Task tool for invoking other agents.
- `primary-only`: Restricts the agent to run only as a primary. Cannot be invoked as a subagent by other agents.

## Task Tool (Registry-Driven Flow)

The Task tool allows agents to invoke other agents with permission-based access control.
This crate provides the [`TaskInput`] and [`TaskOutput`] types used by framework-specific
Task tools. The Task tool behavior is implemented in framework adapters (rig and serdesAI).

### Registry-Driven Task Flow

The new flow for using Task tools is:

1. **Load agent configs** into [`AgentCatalog`] using [`AgentLoader`]
2. **Build a framework registry** using `AgentRegistryBuilder` (rig or serdesAI)
3. **Construct `TaskTool`** from the registry and caller permission rules

#### Example for rig:

See `examples/rig-agents.rs` for the complete example.

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, Ruleset, Rule, PermissionAction};
use llm_coding_tools_rig::{AgentDefaults, AgentRegistryBuilder, TaskTool, default_tools, TodoState};
use rig::providers::openrouter;
use std::sync::Arc;

// 1) Load agent configs
let mut catalog = AgentCatalog::new();
AgentLoader::new().add_directory(&mut catalog, "/home/user/.opencode")?;

// 2) Build framework registry
let client = openrouter::Client::new("OPENROUTER_API_KEY")?;
let defaults = AgentDefaults {
    model: "z-ai/glm-4.5-air:free".into(),
    temperature: None,
    top_p: None,
    options: Default::default(),
};
let tools = default_tools(true, None, TodoState::new());
let builder = AgentRegistryBuilder::new(|model| client.agent(model), defaults, tools);
let registry = builder.build(&catalog)?;

// 3) Create Task tool
let mut rules = Ruleset::new();
rules.push(Rule::new("task", "*", PermissionAction::Allow));
let task_tool = TaskTool::new(Arc::new(registry), rules);
# Ok::<(), Box<dyn std::error::Error>>(())
```

#### Example for serdesAI:

See `examples/serdesai-agents.rs` for the complete example.

```rust,no_run
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, Ruleset, Rule, PermissionAction};
use llm_coding_tools_serdesai::{AgentDefaults, AgentRegistryBuilder, TaskTool, default_tools, TodoState};
use std::sync::Arc;

// 1) Load agent configs
let mut catalog = AgentCatalog::new();
AgentLoader::new().add_directory(&mut catalog, "/home/user/.opencode")?;

// 2) Build framework registry
let defaults = AgentDefaults {
    model: "openrouter:z-ai/glm-4.5-air:free".into(),
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

### Task Input / Output Types

**`TaskInput`**: Input structure for task execution
- `description`: Short (3-5 words) description of the task
- `prompt`: The task for the agent to perform
- `subagent_type`: The type/name of the agent to invoke
- `session_id`: Optional session ID for continuation
- `command`: Optional command that triggered this task

**`TaskOutput`**: Output structure from task execution
- `summary`: The text response from the agent
- `session_id`: Session ID for continuation (if supported)
- `metadata`: Optional execution metadata

### Permission Enforcement

The framework `TaskTool` implementations enforce access validation:

1. Checks if the agent exists (returns validation error if not)
2. Verifies the agent is invocable (not primary-only mode)
3. Checks caller's `task` permission for the requested agent
4. Uses the agent's permission rules to filter available tools

Framework registries precompute allowed tools based on each agent's permission rules
during registry construction.

## Migration from Legacy APIs

The legacy `TaskRunner`, `TaskToolCore`, and `SubagentRegistry` types have been removed.
Migrate to the new flow as follows:

| Legacy API | New API |
|------------|---------|
| `SubagentRegistry` | `AgentCatalog` + framework `AgentRegistry` |
| `TaskRunner` | Not needed - use registry-driven `TaskTool` |
| `TaskToolCore` | Not needed - use framework `TaskTool` types |
| `TaskError` | Framework-specific error types |

For complete migration examples, see:
- `examples/rig-agents.rs` (PROMPT-06)
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
