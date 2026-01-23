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
