# llm-coding-tools-agents

[![Crates.io](https://img.shields.io/crates/v/llm-coding-tools-agents.svg)](https://crates.io/crates/llm-coding-tools-agents) [![Docs.rs](https://docs.rs/llm-coding-tools-agents/badge.svg)](https://docs.rs/llm-coding-tools-agents)

Load agent markdown files compatible with the [OpenCode agent schema]:

- **Mostly drop-in** - agent files work out of the box
- **One exception** - [default-deny permissions](#️-default-deny-permissions) (OpenCode uses default-allow)

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

Agent files are markdown with YAML frontmatter.

The format is based on OpenCode's agent schema; so fields like [`mode`], 
[`model`] and [`permissions`] should be familiar.

### Complete example

```markdown
---
name: code-searcher
mode: subagent
description: Searches codebases to find relevant files and extracts content
model: synthetic/hf:moonshotai/Kimi-K2.5
permission:
  read: allow
  grep: allow
  task: deny
tool_settings:
  read:
    line_numbers: false
  grep:
    line_numbers: false
---

You are a code search assistant. Use grep to find relevant files and code patterns,
then read the matching files to extract and summarize the content.
```

### ⚠️ Default-Deny Permissions

Unlike OpenCode, this library **denies tools unless explicitly allowed**. 

This is because `llm-coding-tools` was designed towards automation/servers,
where determinism is more valuable.

For default-allow behaviour, [open a PR].

### Frontmatter fields

**Required:**
- `description` - What this agent does

**Optional:**
- `name` - Agent identifier (defaults to filename)
- `mode` - Agent behaviour mode
- `model` - LLM provider/model specification
- `permission` - Tool access permissions
- `tool_settings` - Per-tool configuration
- `temperature`, `top_p` - Sampling parameters

#### Mode

- `all` (default) - Both primary and subagent capabilities
- `primary` - Top-level agent, can delegate to subagents
- `subagent` - Can only be invoked by other agents via `task` tool

#### Model

Specify which LLM to use.

Format: `provider/model` or `synthetic/hf:model-id`.

Examples:
- `openai/gpt-5.3-codex`
- `synthetic/hf:moonshotai/Kimi-K2.5`
- `fireworks/accounts/fireworks/routers/kimi-k2p5-turbo`

**Tip:** Use the `llm-coding-tools-models-dev` crate for [models.dev] support.
         You can find examples using it in main repo.

#### Permissions

Map of tool names to `allow` or `deny`. Unlisted tools are denied.

```yaml
permission:
  read: allow
  write: deny
  bash: allow
  task: allow  # Required to delegate to subagents
```

**Note:** `task` is special - when omitted, it allows delegation to all callable
subagents for OpenCode compatibility. To disable delegation, explicitly set
`task: deny`.

#### Tool settings

Configure per-tool behaviour via `tool_settings`:

```yaml
tool_settings:
  read:
    line_numbers: true          # default: true
    limit: 2000                 # default: 2000
    max_line_length: 2000       # default: 2000
  grep:
    line_numbers: true          # default: true
    limit: 100                  # default: 100
    max_line_length: 2000       # default: 2000
  glob:
    limit: 1000                 # default: 1000
  bash:
    timeout_ms: 120000          # default: 120000 (2 minutes)
    max_timeout_ms: 600000      # default: 600000 (10 minutes)
  webfetch:
    timeout_ms: 30000           # default: 30000 (30 seconds)
    max_timeout_ms: 600000      # default: 600000 (10 minutes)
    max_response_size: 5242880  # default: 5242880 (5 MiB in bytes)
```

**Setting reference:**

| Tool     | Setting                 | Type  | Default  | Min  | Description                                             |
| -------- | ----------------------- | ----- | -------- | ---- | ------------------------------------------------------- |
| read     | `line_numbers`          | bool  | `true`   | —    | Show line numbers in output                             |
| read     | `limit`                 | usize | `2000`   | 1    | Max lines per file read                                 |
| read     | `max_line_length`       | usize | `2000`   | 4    | Max characters per line (truncates longer lines)        |
| grep     | `line_numbers`          | bool  | `true`   | —    | Show line numbers in output                             |
| grep     | `limit`                 | usize | `100`    | 1    | Max matches returned                                    |
| grep     | `max_line_length`       | usize | `2000`   | 4    | Max characters per match line                           |
| glob     | `limit`                 | usize | `1000`   | 1    | Max files returned                                      |
| bash     | `timeout_ms`            | usize | `120000` | 1000 | Default command timeout in milliseconds                 |
| bash     | `max_timeout_ms`        | usize | `600000` | *    | Maximum timeout LLM can request (must be >= timeout_ms) |
| webfetch | `timeout_ms`            | usize | `30000`  | 1000 | Fetch timeout in milliseconds                           |
| webfetch | `max_timeout_ms`        | usize | `600000` | *    | Maximum timeout LLM can request (must be >= timeout_ms) |
| webfetch | `max_response_size`     | usize | `5242880`| 1    | Max response body size in bytes                         |

**Output format:**

With line numbers (default `true`):
```text
L1: fn main() {
L2:     println!("Hello");
L3: }
```

Without line numbers (`false`):
```text
fn main() {
    println!("Hello");
}
```

**When to use:**

- **`line_numbers: true`** (default) - When the agent needs to reference specific
  lines, use the `edit` tool, or do code review. Most agents should use this.
  
- **`line_numbers: false`** - For read-only agents that summarize, analyse, or answer
  questions without citing line numbers. Saves tokens and produces cleaner output.

- **`read.limit`** - Maximum number of lines returned when the LLM doesn't specify a
  `limit` in its tool call. Lines beyond this are not read.

- **`read.max_line_length`** - Maximum characters per line in read output. Longer lines
  are truncated with `...` appended.

- **`grep.limit`** - Maximum number of matches returned when the LLM doesn't specify a
  `limit`. Extra matches are dropped.

- **`grep.max_line_length`** - Maximum characters per line in grep output. Longer lines
  are truncated with `...` appended.

- **`glob.limit`** - Maximum number of file paths returned. Results beyond this are
  dropped and `truncated: true` is set.

- **`bash.timeout_ms`** - Maximum time a shell command may run before being killed, in
  milliseconds. Used when the LLM doesn't specify `timeout_ms`.

- **`bash.max_timeout_ms`** - Maximum timeout the LLM is allowed to request via the 
  `timeout_ms` parameter. Must be greater than or equal to `timeout_ms`.

- **`webfetch.timeout_ms`** - Maximum time to wait for a response from a URL, in
  milliseconds. Used when the LLM doesn't specify `timeout_ms`.

- **`webfetch.max_timeout_ms`** - Maximum timeout the LLM is allowed to request via the
  `timeout_ms` parameter. Must be greater than or equal to `timeout_ms`.

- **`webfetch.max_response_size`** - Maximum response body size in bytes.
  Responses larger than this are rejected. Default is 5242880 bytes (5 MiB).

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

- `permission.task: ask` - Rejected with a schema validation error (`allow`/`deny`
  only), because `ask` is an interactive approval mode in OpenCode.
- `hidden` - Accepted for compatibility, but ignored at runtime.

For the internal architecture, see [ARCHITECTURE.md](https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-agents/ARCHITECTURE.md).

[`mode`]: https://opencode.ai/docs/agents#mode
[`model`]: https://opencode.ai/docs/agents#model
[`permissions`]: https://opencode.ai/docs/agents#permissions
[models.dev]: https://models.dev
[OpenCode agent schema]: https://opencode.ai/docs/agents/
[open a PR]: https://github.com/Sewer56/llm-coding-tools/pulls
