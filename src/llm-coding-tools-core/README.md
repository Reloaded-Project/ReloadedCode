# llm-coding-tools-core

Framework-agnostic core library of standard tools used by coding agents - headless, TUI, or anything in between.

`llm-coding-tools-core` provides reviewed, production-grade implementations of common coding-agent tools, plus shared safety, prompt, and policy primitives.

## Table of contents

- [Install](#install)
- [Feature flags](#feature-flags)
- [Tools, context, and integration](#tools-context-and-integration)
- [System prompt builder](#system-prompt-builder)
- [Permissions](#permissions)

## Install

```toml
# Async (default)
llm-coding-tools-core = "0.2"

# Sync/blocking
llm-coding-tools-core = { version = "0.2", default-features = false, features = ["blocking"] }
```

## Feature flags

- `tokio` (default): async runtime support
- `blocking`: sync/blocking mode
- `async`: internal base async feature (enabled by runtimes, not directly)

`tokio` and `blocking` are mutually exclusive.

## Tools, context, and integration

Canonical tool names are defined in [`tool_names`] ([`read`], [`write`], [`edit`], [`glob`], [`grep`], [`bash`], [`webfetch`], [`todoread`], [`todowrite`], [`task`]).

### Standard tools

- [`read`] ([`read_file`]) - Read a file window (`offset`/`limit`) with const-generic line numbers (`read_file::<_, true>` or `read_file::<_, false>`).
- [`write`] ([`write_file`]) - Create or overwrite a file at a resolved path.
- [`edit`] ([`edit_file`]) - Apply exact text replacements with structured edit errors.
- [`glob`] ([`glob_files`]) - Match filesystem paths by glob pattern.
- [`grep`] ([`grep_search`]) - Search file contents by regex with match metadata.
- [`bash`] ([`execute_command`]) - Execute shell commands with timeout and captured output.
- [`webfetch`] ([`fetch_url`]) - Fetch URL content as text, markdown, or html (requires `tokio` or `blocking`).
- [`todoread`] ([`read_todos`]) - Read shared todo state.
- [`todowrite`] ([`write_todos`]) - Write and validate shared todo state.
- [`task`] ([`TaskInput`], [`TaskOutput`]) - Standard task payload types used by delegation wrappers.

### Path safety and sandboxing

Path-based tools are generic over [`PathResolver`], so wrappers can choose unrestricted access or sandboxed access.

- [`AbsolutePathResolver`] enforces absolute-path inputs (unrestricted mode).
- [`AllowedPathResolver`] constrains operations to configured directories (sandbox mode).
- Failed resolution rejects traversal and out-of-sandbox paths before tool execution.

```rust,no_run
use llm_coding_tools_core::{AbsolutePathResolver, AllowedPathResolver, PathResolver, ToolResult};

fn demo() -> ToolResult<()> {
    // Unrestricted mode: any absolute path is allowed.
    let any_path = AbsolutePathResolver;
    let _hosts = any_path.resolve("/etc/hosts")?;

    // Sandboxed mode: only configured directories are allowed.
    let sandbox = AllowedPathResolver::new(["/workspace/project", "/tmp"])?;
    let _lib = sandbox.resolve("src/lib.rs")?;
    Ok(())
}
```

### Context and wrapper mapping

[`context`] provides reusable guidance constants.

Wrappers usually bind a tool's canonical name and guidance through [`ToolContext`]:

Any-path read tool:

```rust,no_run
use llm_coding_tools_core::{ToolContext, context, tool_names};

struct ReadTool;

impl ReadTool {
    fn new() -> Self {
        Self
    }
}

impl ToolContext for ReadTool {
    const NAME: &'static str = tool_names::READ;

    fn context(&self) -> &'static str {
        context::READ_ABSOLUTE
    }
}

let _tool = ReadTool::new();
```

Sandboxed read tool:

```rust,no_run
use llm_coding_tools_core::{AllowedPathResolver, ToolContext, context, tool_names};

struct ReadTool {
    _resolver: AllowedPathResolver,
}

impl ReadTool {
    fn new(resolver: AllowedPathResolver) -> Self {
        Self {
            _resolver: resolver,
        }
    }
}

impl ToolContext for ReadTool {
    const NAME: &'static str = tool_names::READ;

    fn context(&self) -> &'static str {
        context::READ_ALLOWED
    }
}

let resolver = AllowedPathResolver::new(["/workspace/project"]).expect("valid allowed path");
let _tool = ReadTool::new(resolver);
```

Core tool functions are generic over [`PathResolver`], but wrappers usually expose separate absolute/allowed tool types for simpler ergonomics (to avoid extra generic parameters).

This keeps registration name (`read`) and prompt guidance in sync.

## System prompt builder

[`SystemPromptBuilder`] builds one prompt string for agent runtimes.

- [`track(&mut self, tool: T)`] records tool guidance and returns the tool unchanged.
- [`working_directory(self, path)`] and [`allowed_paths(self, resolver)`] add environment metadata.
- [`add_context(self, name, context)`] appends supplemental sections (for example `GIT_WORKFLOW`).
- [`system_prompt(self, prompt)`] prepends custom instructions; [`build(self)`] renders the final prompt.

You usually build framework wrappers from these primitives (`ToolContext` + `SystemPromptBuilder`).

### Typical wrapper integration (serdesAI)

For example with `llm-coding-tools-serdesai`, wrappers are built from these primitives.

```rust,no_run
# #[cfg(any())]
# {
use llm_coding_tools_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_serdesai::{BashTool, SystemPromptBuilder};
use serdes_ai::prelude::*;

let mut pb = SystemPromptBuilder::new()
    .working_directory(std::env::current_dir()?.display().to_string());

let agent = AgentBuilder::<(), String>::new(model)
    .tool(pb.track(ReadTool::<true>::new()))
    .tool(pb.track(GlobTool::new()))
    .tool(pb.track(GrepTool::<true>::new()))
    .tool(pb.track(BashTool::new()))
    .system_prompt(pb.build())
    .build();
# }
```

## Permissions

[`permissions`] provides ordered allow/deny rules for tool access and delegation.

- [`Rule`] stores `(permission_key, subject_pattern, action)`.
- [`Ruleset`] uses last-match-wins; no match defaults to [`PermissionAction::Deny`].
- Permission keys are exact-match and case-sensitive; wildcard matching (`*`, `?`) applies to subject patterns.

Frontmatter-style config is typically translated into this model:

```yaml
permission:
  bash: allow
  task:
    orchestrator-*: allow
    "*": deny
```

With last-match-wins, the final `"*": deny` rule overrides earlier `task` matches.

```rust
use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};

let mut rules = Ruleset::new();
rules.push(Rule::new("bash", "*", PermissionAction::Allow));
rules.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));
rules.push(Rule::new("task", "*", PermissionAction::Deny));

assert_eq!(rules.evaluate("bash", "any-agent"), PermissionAction::Allow);
assert_eq!(rules.evaluate("task", "orchestrator-review"), PermissionAction::Deny); // last-match-wins
```

[`tool_names`]: crate::tool_names
[`read`]: crate::tool_names::READ
[`write`]: crate::tool_names::WRITE
[`edit`]: crate::tool_names::EDIT
[`glob`]: crate::tool_names::GLOB
[`grep`]: crate::tool_names::GREP
[`bash`]: crate::tool_names::BASH
[`webfetch`]: crate::tool_names::WEBFETCH
[`todoread`]: crate::tool_names::TODO_READ
[`todowrite`]: crate::tool_names::TODO_WRITE
[`task`]: crate::tool_names::TASK
[`read_file`]: crate::read_file
[`write_file`]: crate::write_file
[`edit_file`]: crate::edit_file
[`glob_files`]: crate::glob_files
[`grep_search`]: crate::grep_search
[`execute_command`]: crate::execute_command
[`fetch_url`]: crate::fetch_url
[`read_todos`]: crate::read_todos
[`write_todos`]: crate::write_todos
[`TaskInput`]: crate::TaskInput
[`TaskOutput`]: crate::TaskOutput
[`SystemPromptBuilder`]: crate::SystemPromptBuilder
[`track(&mut self, tool: T)`]: crate::SystemPromptBuilder::track
[`working_directory(self, path)`]: crate::SystemPromptBuilder::working_directory
[`allowed_paths(self, resolver)`]: crate::SystemPromptBuilder::allowed_paths
[`add_context(self, name, context)`]: crate::SystemPromptBuilder::add_context
[`system_prompt(self, prompt)`]: crate::SystemPromptBuilder::system_prompt
[`build(self)`]: crate::SystemPromptBuilder::build
[`context`]: crate::context
[`ToolContext`]: crate::context::ToolContext
[`PathResolver`]: crate::PathResolver
[`AbsolutePathResolver`]: crate::AbsolutePathResolver
[`AllowedPathResolver`]: crate::AllowedPathResolver
[`permissions`]: crate::permissions
[`Rule`]: crate::permissions::Rule
[`Ruleset`]: crate::permissions::Ruleset
[`PermissionAction::Deny`]: crate::permissions::PermissionAction::Deny
