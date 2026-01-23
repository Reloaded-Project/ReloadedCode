# llm-coding-tools-subagents

Subagent configuration loading from OpenCode-style markdown files with YAML frontmatter.

## Features

- Parse markdown files with YAML frontmatter
- Preprocess frontmatter to handle inline colons (e.g., `model: provider/model:tag`)
- Scan directories for agent configs matching `agent/**/*.md` and `agents/**/*.md`
- Derive agent names from file paths

## Usage

```rust
use llm_coding_tools_subagents::{load_agents, AgentConfig};
use std::path::Path;

let agents = load_agents(&[Path::new("~/.opencode")]).unwrap();
for (name, config) in &agents {
    println!("{}: {}", name, config.description);
}
```

## Agent File Format

```markdown
---
mode: subagent
description: Explores codebase structure
model: provider/model-id
permission:
  read: allow
  task: deny
---

Prompt body goes here...
```

## Task Tool

The Task tool allows agents to invoke subagents with permission-based access control.

### Core Components

- `TaskInput` / `TaskOutput` - Input/output types for task execution
- `TaskError` - Error types for task failures
- `TaskRunner` - Trait for framework-specific execution
- `TaskToolCore` - Enforces access validation before delegating to runner

### Usage with Framework Adapters

Framework adapters (rig, serdesAI) wrap `TaskToolCore`:

```rust
use llm_coding_tools_subagents::{TaskToolCore, TaskRunner, Ruleset};
use std::sync::Arc;

// Create runner (framework-specific implementation)
let runner: Arc<MyRunner> = /* ... */;

// Create core with caller's permission rules
let core = TaskToolCore::new(runner, caller_rules);

// Build description for tool definition
let description = core.build_description();

// Execute with enforced access validation
let result = core.execute(input, &deps).await?;
```

### Permission Enforcement

Access validation is ALWAYS enforced in `TaskToolCore::execute`:

1. Checks if the agent exists (returns `UnknownAgent` if not)
2. Verifies the subagent is invocable (not primary-only)
3. Checks caller's `task` permission for the requested subagent
4. Computes allowed tools via the subagent's permission rules
5. Passes `allowed_tools` to the runner

### Tool Filtering

The runner receives `allowed_tools` computed by `TaskToolCore`:

1. Gets subagent's available tools from `agent_tools()`
2. Gets subagent's permission rules from `agent_rules()`
3. Filters tools by `is_allowed(tool_name, "*")`
4. Preserves original tool name casing (normalizes only for comparison)

### serdesAI Implementation Note

When implementing `TaskRunner` for serdesAI, use `AgentBuilderExt::tool` and filter by `allowed_tools`:

```rust
use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;

// In TaskRunner::run implementation:
let mut builder = AgentBuilder::<Deps, String>::from_model(&config.model)?;
for tool in available_tools {
    if allowed_tools.iter().any(|t| t.eq_ignore_ascii_case(&tool.name())) {
        builder = builder.tool(tool);
    }
}
```
