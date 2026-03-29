# Architecture: llm-coding-tools-agents

Framework-agnostic agent configuration loading, catalog management, model
resolution, permission filtering, and runtime assembly.

Upstream integrations (e.g. `llm-coding-tools-serdesai`) consume the
[`AgentRuntime`] produced here and adapt it to their framework's agent builder.

## Table of Contents

- [Quick Start](#quick-start)
- [Phase 1: Loading](#phase-1-loading)
  - [Loading Pipeline](#loading-pipeline)
  - [File Discovery](#file-discovery)
  - [YAML Preprocessor](#yaml-preprocessor)
- [Phase 2: Building](#phase-2-building)
  - [Building the Runtime](#building-the-runtime)
  - [AgentDefaults](#agentdefaults)
  - [Tool Catalog](#tool-catalog)
  - [Model Resolution](#model-resolution)
- [Phase 3: Runtime Usage](#phase-3-runtime-usage)
  - [Permission Filtering](#permission-filtering)
  - [Allowed Tools](#allowed-tools)
  - [Callable Targets](#callable-targets)
- [Reference](#reference)
  - [Error Model](#error-model)
  - [Testing](#testing)
  - [File Map](#file-map)

## Quick Start

Three steps from markdown files to a working runtime:

```rust
use llm_coding_tools_agents::{AgentLoader, AgentCatalog, AgentRuntimeBuilder, AgentDefaults};
use std::path::Path;

// 1. Load agents from directory
let loader = AgentLoader::new();
let mut catalog = AgentCatalog::new();
loader.add_directory(&mut catalog, Path::new("agents"))?;

// 2. Build the runtime
let runtime = AgentRuntimeBuilder::new()
    .catalog(catalog)
    .defaults(AgentDefaults::with_model("openai/gpt-4o"))
    .build();

// 3. Use the runtime (e.g., look up agents by name)
let agent = runtime.catalog().by_name("code-reviewer").unwrap();
// ... pass to your framework's agent builder
```

## Phase 1: Loading

Agent definitions live in markdown files with YAML frontmatter:

```markdown
---
name: code-reviewer
mode: subagent
description: Reviews code and flags high-risk issues
model: openrouter/openai/gpt-4o
permission:
  read: allow
  bash: deny
  task:
    "*": deny
    review-*: allow
---
You are a careful code reviewer.
```

### Loading Pipeline

```text
    .md file / string / bytes
             │
             │  AgentLoader::add_directory / add_file / add_from_str
             ▼
    ┌─────────────────────────────────────────────────────────────────────┐
    │ 1. CRLF -> LF normalization             crlf-to-lf-inplace          │
    │ 2. Find frontmatter delimiters          parser/mod.rs               │
    │ 3. Preprocess YAML                      preprocessor                │
    │    Rewrites colon-containing values to block scalars                │
    │ 4. Parse YAML -> serde_yaml::Value      serde_yaml                  │
    │ 5. Validate headless compatibility      no "ask" in permission.task │
    │ 6. Deserialize Value -> RawFrontmatter  serde_yaml                  │
    │ 7. Build AgentConfig                    from_raw                    │
    └──────────────────────────────┬──────────────────────────────────────┘
                                   │
                                   ▼
                            ┌─────────────┐
                            │ AgentConfig │  name, mode, model, permissions, prompt
                            └──────┬──────┘
                                   │
                                   ▼
                            ┌──────────────┐
                            │ AgentCatalog │  AHashMap<String, AgentConfig>
                            └──────────────┘  last-insert-wins on duplicate names
```

### File Discovery

`AgentLoader::add_directory` walks the given root with `.gitignore` support
(`ignore` crate), keeping only files matching:

```text
agent/**/*.md
agents/**/*.md
```

Agent name is derived from the relative path by stripping the `agent/` or
`agents/` prefix and `.md` suffix:

```text
agent/code-reviewer.md      -> "code-reviewer"
agents/nested/deep.md        -> "nested/deep"
```

Frontmatter `name:` overrides the derived name when present.

### YAML Preprocessor

The preprocessor (`parser/preprocessor.rs`) rewrites lines where an unquoted
value contains a bare `:` - a YAML ambiguity. For example:

```yaml
model: provider/model:tag
```

becomes:

```yaml
model: |-
  provider/model:tag
```

Already-safe forms (quoted, block scalars, flow syntax, comments, indented
continuation lines) are left untouched.

## Phase 2: Building

Once you have an [`AgentCatalog`], you assemble an [`AgentRuntime`] that holds
everything needed to run agents: the catalog, default settings, Task delegation
settings, and the available tools.

### Building the Runtime

```text
   AgentRuntimeBuilder::new()
       .catalog(catalog)
       .defaults(AgentDefaults { model, temperature, top_p })
       .max_task_depth(n)        // or .task_settings(TaskSettings::with_max_depth(n))
       .tools(vec![...])         // or default_tools() if omitted
       .build()
                │
                ▼
          ┌──────────────┐
          │ AgentRuntime │  catalog + defaults + task_settings + tools
          └──────────────┘
```

`AgentRuntime` is `Clone`, `Send`, `Sync`, and stores no async state.

### AgentDefaults

Fallback settings used when an individual agent doesn't specify them:

| Field         | Meaning                            |
| ------------- | ---------------------------------- |
| `model`       | Default `provider/model-id`        |
| `temperature` | Default sampling temperature       |
| `top_p`       | Default nucleus sampling parameter |

### Tool Catalog

`default_tools()` returns 10 entries:

| Kind      | Tool name   |
| --------- | ----------- |
| Read      | `read`      |
| Write     | `write`     |
| Edit      | `edit`      |
| Glob      | `glob`      |
| Grep      | `grep`      |
| Bash      | `bash`      |
| WebFetch  | `webfetch`  |
| TodoRead  | `todoread`  |
| TodoWrite | `todowrite` |
| Task      | `task`      |

### Model Resolution

When an agent needs to run, you resolve which model it should use:

```text
   resolve_model_with_catalog(model_catalog, defaults, agent)
                │
                │  1. agent.model set?  -> parse "provider/model-id"
                │     └─ malformed?     -> MalformedModelIdentifier ("agent override")
                │
                │  2. defaults.model?   -> parse "provider/model-id"
                │     └─ malformed?     -> MalformedModelIdentifier ("runtime default")
                │
                │  3. neither set?       -> MissingEffectiveModel
                │
                │  4. provider in catalog?  -> no -> UnknownProvider
                │  5. model in catalog?     -> no -> UnknownModel
                ▼
          ┌────────────────┐
          │ ResolvedModel  │  provider: Box<str>, model: Box<str>
          └────────────────┘
```

Precedence: **agent override** wins over **runtime default**.

A malformed agent override does **not** fall back to the default - it errors.

## Phase 3: Runtime Usage

With a built [`AgentRuntime`], you can query what an agent is allowed to do
and which other agents it can delegate to.

### Permission Filtering

Agent frontmatter may include a `permission` map:

```yaml
permission:
  read: allow
  bash: deny
  task:
    "*": deny
    "review-*": allow
```

`RulesetExt::from_permission_config` converts this into a `Ruleset` (from
`llm-coding-tools-core::permissions`):

```text
PermissionRule::Action(Allow)    -> Rule { key: "read",   pattern: "*", action: Allow }
PermissionRule::Action(Deny)     -> Rule { key: "bash",   pattern: "*", action: Deny  }
PermissionRule::Pattern({ .. })  -> Rule { key: "task",   pattern: "*",        action: Deny  }
                                    Rule { key: "task",   pattern: "review-*", action: Allow }
```

Evaluation uses **last-match-wins** semantics.

### Allowed Tools

`AgentRuntime::allowed_tools(caller_name)` filters the tool catalog:

```text
   runtime.tools()
        │
        │  for each entry:
        │    Task  -> only if >= 1 callable subagent target exists
        │    other -> is_allowed(entry.name, "*") per Ruleset
        ▼
   Vec<ToolCatalogEntry>
```

### Callable Targets

`callable_targets(catalog, caller_name)` returns agents the caller may delegate
to via the Task tool:

```text
   all agents (sorted by name)
        │
        │  filter:
        │    mode != Primary  (only All + Subagent are callable)
        │    AND
        │    if caller defines permission.task:
        │      ruleset.is_allowed("task", target.name)
        │    else (no explicit permission.task):
        │      default-allow all non-Primary targets
        ▼
   Vec<&AgentConfig>
```

OpenCode compatibility: omitting `permission.task` defaults to allowing
delegation to all non-Primary agents.

## Reference

### Error Model

```text
AgentLoadError
├── Io { path, source }              file read / directory scan failure
├── Parse { path, source }           frontmatter YAML parse failure
│                                     source: AgentParseError
│                                       ├── MissingFrontmatter
│                                       ├── InvalidYaml { message }
│                                       └── SchemaValidation { message }
└── SchemaValidation { path, message } invalid mode, empty name, "ask" permission

ModelResolutionError
├── MalformedModelIdentifier          missing "/" or empty segments
├── MissingEffectiveModel             neither agent nor default specifies a model
├── UnknownProvider                   provider not in ModelCatalog
└── UnknownModel                      provider found but model not listed
```

All loader errors carry an optional `path: Option<PathBuf>` (`None` for
in-memory sources, displayed as `<memory>`).

### Testing

- `tempfile` + `indoc` fixtures for file/directory loading tests.
- No external services required.
- Parser benchmarks in `benches/parser.rs` (Criterion).

### File Map

```text
llm-coding-tools-agents
├── lib.rs                  crate root, re-exports
├── catalog.rs              AgentCatalog - in-memory name -> AgentConfig store
├── extensions.rs           RulesetExt - builds Ruleset from frontmatter permissions
├── loader.rs               AgentLoader - scans dirs/files/strings -> AgentCatalog
├── parser/
│   ├── mod.rs              parse_agent() - YAML frontmatter + body extractor
│   └── preprocessor.rs     YAML preprocessor - rewrites colon-containing values
├── types/
│   ├── mod.rs              re-exports
│   ├── config.rs           AgentConfig, AgentMode, PermissionRule, parse_model_parts
│   ├── error.rs            AgentLoadError, AgentLoadResult
│   └── tool_settings.rs    AgentToolSettings, ReadToolSettings, GrepToolSettings
├── runtime/
│   ├── mod.rs              module root, re-exports
│   ├── state.rs            AgentRuntime, AgentDefaults
│   ├── builder.rs          AgentRuntimeBuilder
│   ├── model.rs            resolve_model_with_catalog(), ResolvedModel, ModelResolutionError
│   ├── task.rs             callable_targets(), summarize_callable_targets(), allowed_tools()
│   └── tool_catalog.rs     ToolCatalogEntry, ToolCatalogKind, default_tools()
└── benches/
    └── parser.rs           Criterion benchmarks for frontmatter parsing
```
