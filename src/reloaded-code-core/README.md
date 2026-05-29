# reloaded-code-core

[![Crates.io](https://img.shields.io/crates/v/reloaded-code-core.svg)](https://crates.io/crates/reloaded-code-core) [![Docs.rs](https://docs.rs/reloaded-code-core/badge.svg)](https://docs.rs/reloaded-code-core)

Framework-agnostic core tools for building coding agents - file operations,
search, shell execution, sandboxing, permissions, and system prompt generation.

Headless, TUI, or anything in between. Production-grade implementations with minimal overhead.

[Documentation] · [API Reference]

## Table of contents

- [reloaded-code-core](#reloaded-code-core)
  - [Table of contents](#table-of-contents)
  - [Install](#install)
  - [Feature flags](#feature-flags)
  - [Tools, context, and integration](#tools-context-and-integration)
    - [Standard tools](#standard-tools)
    - [Path safety and sandboxing](#path-safety-and-sandboxing)
      - [Linux shell sandboxing](#linux-shell-sandboxing)
    - [Context and wrapper mapping](#context-and-wrapper-mapping)
  - [System prompt builder](#system-prompt-builder)
    - [Typical wrapper integration (serdesAI)](#typical-wrapper-integration-serdesai)
  - [Permissions](#permissions)
  - [Credentials](#credentials)

## Install

```toml
# Async (default)
reloaded-code-core = "0.2"

# Sync/blocking
reloaded-code-core = { version = "0.2", default-features = false, features = ["blocking"] }
```

## Feature flags

- `tokio` (default): async runtime support
- `blocking`: sync/blocking mode
- `async`: internal base async feature (enabled by runtimes, not directly)
- `linux-bubblewrap`: Sandboxing support for Linux, by leveraging `bwrap` tool.

`tokio` and `blocking` are mutually exclusive.

## Tools, context, and integration

Canonical tool metadata lives in [`tool_metadata`].

Each grouped module exposes the model-facing tool name plus the provider-facing
metadata used by wrappers such as [SerdesAI]: [`read`], [`write`], [`edit`],
[`glob`], [`grep`], [`bash`], [`webfetch`], [`todoread`], [`todowrite`],
and [`task`].

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
- [`task`] ([`TaskInput`], [`TaskOutput`], [`TaskSettings`]) - Standard task payload types and shared delegation limits used by runtime wrappers.

### Path safety and sandboxing

Path-based tools are generic over [`PathResolver`], so wrappers can choose unrestricted access or sandboxed access.

- [`AbsolutePathResolver`] enforces absolute-path inputs (unrestricted mode).
- [`AllowedPathResolver`] constrains operations to configured directories (sandbox mode).
- [`AllowedGlobResolver`] constrains to a workspace root with glob pattern filtering (fine-grained sandbox mode).
- Failed resolution rejects traversal and out-of-sandbox paths before tool execution.

```rust,no_run
use reloaded_code_core::{
    path::{AllowedGlobResolver, GlobPolicy, RuleAction},
    resolve_workspace_root, AbsolutePathResolver, AllowedPathResolver, PathResolver, ToolResult,
};

fn demo() -> ToolResult<()> {
    // Unrestricted mode: any absolute path is allowed.
    let any_path = AbsolutePathResolver;
    let _hosts = any_path.resolve("/etc/hosts")?;

    // Sandboxed mode: multiple allowed directories.
    let sandbox = AllowedPathResolver::new(["/workspace/project", "/tmp"])?;
    let _lib = sandbox.resolve("src/lib.rs")?;

    // Fine-grained sandbox with glob policy (workspace-relative patterns).
    let root = resolve_workspace_root()?;
    let glob = AllowedGlobResolver::new(&root)?
        .with_policy(
            GlobPolicy::builder()
                .add("src/**", RuleAction::Allow)?    // Matches src/lib.rs
                .add("*.rs", RuleAction::Allow)?      // Also matches src/lib.rs
                .add("target/**", RuleAction::Deny)?  // Blocks target/ even if *.rs matches
                .build()?
        );
    let _lib = glob.resolve("src/lib.rs")?;
    let _main = glob.resolve("main.rs")?;
    // glob.resolve("target/debug/app")?; // Denied
    Ok(())
}
```

#### Permission glob semantics

- `*` matches any characters within a single path component (e.g., `*.rs` matches `lib.rs` but not `src/lib.rs`).
- `**` matches any number of path components, relative to the workspace root (e.g., `src/**/*.rs` matches `src/deep/nested/mod.rs`).
- `/**` matches any absolute path on the system (e.g., matches `/etc/passwd`, `C:/Windows/system32`).
- Bare `allow` maps to `**` (all files under the workspace root).
- Relative patterns are implicitly joined with the workspace root at construction time.
- Absolute patterns (leading `/` or drive-root like `C:/`) are treated as-is.

Last-match-wins: both deny-then-allow and allow-then-deny orders work depending on whether you want a default-deny or default-allow posture.

#### Linux shell sandboxing

Enable the `linux-bubblewrap` feature flag to sandbox [`bash`] ([`execute_command`])
via Linux `bwrap`. This limits visible filesystem, environment, and network
access for executed commands.

Two profiles are available:

- **Public Bot** (`Profile::public_bot_defaults`)
  Strictest containment for hostile input. No host filesystem access, synthetic
  home, memory-backed `/tmp`, network disabled.

- **Trusted Maintenance** (`Profile::trusted_maintenance_defaults`)
  Broader profile for builds and repairs in a more trusted environment.
  Read-only host `/` with writable overlays, disk-backed `/tmp`, network enabled.

We default to the **Public Bot** profile when sandboxing is enabled. In either
case, evaluate whether the chosen profile fits your security needs.

See [Sandboxing docs](https://reloaded-project.github.io/ReloadedCode/sandboxing/) for the full operator
guide and checklist.

### Context and wrapper mapping

[`context`] provides reusable guidance constants.

Wrappers usually bind a tool's canonical name and guidance through
[`ToolContext`]:

Any-path read tool:

```rust,no_run
use reloaded_code_core::context::{PathMode, ToolPrompt};
use reloaded_code_core::{ToolContext, tool_metadata};

struct ReadTool;

impl ReadTool {
    fn new() -> Self {
        Self
    }
}

impl ToolContext for ReadTool {
    fn name(&self) -> &'static str {
        tool_metadata::read::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Read {
            path_mode: PathMode::Absolute,
            line_numbers: true,
        }
    }
}

let _tool = ReadTool::new();
```

Sandboxed read tool:

```rust,no_run
use reloaded_code_core::{
    AllowedPathResolver, ToolContext, tool_metadata,
};
use reloaded_code_core::context::{PathMode, ToolPrompt};

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
    fn name(&self) -> &'static str {
        tool_metadata::read::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Read {
            path_mode: PathMode::Allowed,
            line_numbers: true,
        }
    }
}

let resolver = AllowedPathResolver::new(["/workspace/project"])
    .expect("valid allowed path");
let _tool = ReadTool::new(resolver);
```

Core tool functions are generic over [`PathResolver`], but wrappers usually
expose separate absolute/allowed tool types for simpler ergonomics.
This avoids extra generic parameters.

This keeps registration names such as `tool_metadata::read::NAME` and prompt
guidance in sync.

## System prompt builder

[`SystemPromptBuilder`] builds one prompt string for agent runtimes.

- [`track(&mut self, tool: T)`] records tool guidance and returns the tool unchanged.
- [`working_directory(self, path)`] and [`allowed_paths(self, resolver)`] add environment metadata.
- [`add_context(self, name, context)`] appends supplemental sections (for example `GIT_WORKFLOW`).
- [`system_prompt(self, prompt)`] prepends custom instructions; [`build(self)`] renders the final prompt.

You usually build framework wrappers from these primitives (`ToolContext` + `SystemPromptBuilder`).

### Typical wrapper integration (serdesAI)

For example with `reloaded-code-serdesai`, wrappers are built from these primitives.

```rust,no_run
# #[cfg(any())]
# {
use reloaded_code_serdesai::absolute::{GlobTool, GrepTool, ReadTool};
use reloaded_code_serdesai::{BashTool, SystemPromptBuilder};
use serdes_ai::prelude::*;

let mut pb = SystemPromptBuilder::new()
    .working_directory(std::env::current_dir()?.display().to_string());

let agent = AgentBuilder::<(), String>::new(model)
    .tool(pb.track(ReadTool::<true>::new()))
    .tool(pb.track(GlobTool::new()))
    .tool(pb.track(GrepTool::<true>::new()))
    .tool(pb.track(BashTool::host()))
    .system_prompt(pb.build())
    .build();
# }
```

To preview the built-in guidelines and their static cost, run the `system_prompt_preview` example (and its variants).

The system prompt is auto-optimized: cross-tool references e.g.
`prefer X tool over Y for Z` are ommitted unless all tools are present.
Currently uses ~2000 tokens for full toolset, ~560 tokens for search-only.

## Permissions

[`permissions`] provides ordered allow/deny rules for tool access and delegation.

- [`Rule`] stores `(permission_key, subject_pattern, action)`.
- [`Ruleset`] uses last-match-wins; no match defaults to [`PermissionAction::Deny`].
- Both fields support patterns:
  - `*` means any number of characters (including none)
  - `?` means exactly one character
  - **Permission keys** (exact or wildcard): `bash`, `task`, `webfetch-*`
  - **Subject patterns** (wildcard only): `*`, `orchestrator-*`, `agent-?`

Frontmatter-style config is typically translated into this model:

```yaml
permission:
  bash: allow              # → ("bash", "*", allow)
  task:
    orchestrator-*: allow  # → ("task", "orchestrator-*", allow)
    "*": deny              # → ("task", "*", deny)
```

With last-match-wins, the final `"*": deny` rule overrides earlier `task` matches.

```rust
use reloaded_code_core::permissions::{ExpandError, PermissionAction, Rule, Ruleset};

# fn main() -> Result<(), ExpandError> {
let mut rules = Ruleset::new();
rules.push(Rule::new("bash", "*", PermissionAction::Allow)?);
rules.push(Rule::new("task", "*", PermissionAction::Deny)?);               // catch-all
rules.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow)?); // specific (last match wins)

assert_eq!(rules.evaluate("bash", "any-agent"), PermissionAction::Allow);
assert_eq!(rules.evaluate("task", "orchestrator-review"), PermissionAction::Allow); // last-match-wins
assert_eq!(rules.evaluate("task", "other-agent"), PermissionAction::Deny); // no match, defaults to deny
# Ok(())
# }
```

## Credentials

[`CredentialResolver`] looks up named credentials (like API keys) from overrides or environment variables.

- [`CredentialResolver::new()`] checks overrides first, then falls back to environment variables.
- [`CredentialResolver::without_env()`] only uses explicit overrides.
- [`set_override`] stores a value that takes precedence over environment variables.

```rust,no_run
use reloaded_code_core::{CredentialLookup, CredentialResolver};

let mut resolver = CredentialResolver::new();
resolver.set_override("OPENAI_API_KEY", "sk-override");
let key = resolver.resolve("OPENAI_API_KEY");
```

[`tool_metadata`]: crate::tool_metadata
[`read`]: crate::tool_metadata::read::NAME
[`write`]: crate::tool_metadata::write::NAME
[`edit`]: crate::tool_metadata::edit::NAME
[`glob`]: crate::tool_metadata::glob::NAME
[`grep`]: crate::tool_metadata::grep::NAME
[`bash`]: crate::tool_metadata::bash::NAME
[`webfetch`]: crate::tool_metadata::webfetch::NAME
[`todoread`]: crate::tool_metadata::todo_read::NAME
[`todowrite`]: crate::tool_metadata::todo_write::NAME
[`task`]: crate::tool_metadata::task::NAME
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
[`TaskSettings`]: crate::TaskSettings
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
[`AllowedGlobResolver`]: crate::path::AllowedGlobResolver
[`AllowedPathResolver`]: crate::AllowedPathResolver
[`permissions`]: crate::permissions
[`Rule`]: crate::permissions::Rule
[`Ruleset`]: crate::permissions::Ruleset
[`PermissionAction::Deny`]: crate::permissions::PermissionAction::Deny
[`CredentialResolver`]: crate::CredentialResolver
[`CredentialResolver::new()`]: crate::CredentialResolver::new
[`CredentialResolver::without_env()`]: crate::CredentialResolver::without_env
[`set_override`]: crate::CredentialResolver::set_override
[SerdesAI]: https://crates.io/crates/serdes-ai
[Documentation]: https://reloaded-project.github.io/ReloadedCode/
[API Reference]: https://docs.rs/reloaded-code-core
