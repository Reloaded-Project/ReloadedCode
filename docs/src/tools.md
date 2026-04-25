# Tools

!!! tip "reloaded-code provides 10 standard tools that cover the core needs of a coding agent."

Every tool has a plain function implementation in [reloaded-code-core]. Adapter
implementations that integrate those functions with LLM frameworks live in crates
like [reloaded-code-serdesai].

Jump to [Tool overview](#tool-overview) for the tool list, or read on for how
configuration and permissions work.

## How it fits together

Tools are configured through [agent files] or in code.

The configuration is best illustrated with an agent file. The example below
covers three concepts - which tools are available, what they may access, and
how defaults are configured:

```yaml
---
name: code-searcher
mode: subagent
description: Searches codebases to find relevant files

# (1) Permissions: which tools the agent can use, and optionally which
#     file paths, commands, or agent names each tool may access.
#     Default-deny: every tool is blocked unless explicitly allowed.
permission:
  read: allow
  glob: allow
  grep: allow
  bash: deny       # explicit deny (same as omitting it)

# (2) Tool settings: host-side defaults for tools that support them.
#     These are NOT per-call parameters - they set the limits the agent
#     operates within. The LLM never sees or overrides these.
tool_settings:
  read:
    line_numbers: false     # omit line numbers (we parse output programmatically)
    limit: 500              # return at most 500 lines per read
  grep:
    line_numbers: false
    limit: 50               # cap search results at 50 matches
---

You are a code search assistant. Use grep to find relevant files, then read
the matching files to extract and summarize the content.
```

| Concept            | Where configured      | What it controls                                                                                          |
| ------------------ | --------------------- | --------------------------------------------------------------------------------------------------------- |
| Availability       | `permission`          | Which tools the agent may call, and optionally which file paths, commands, or agent names they may access |
| Defaults & limits  | `tool_settings`       | Host-side constraints like line counts, timeouts                                                          |
| Per-call behaviour | (LLM-supplied params) | `offset`, `limit` within the host's bounds, etc.                                                          |

See [Agents] for the full agent file specification.

The `permission` field is the most nuanced of the three. Beyond simple
allow/deny, it supports pattern-based rules:

### Permission rules

Permissions use **pattern-based rules** with last-match-wins evaluation. Not
all tools support this:

| Tool(s)                       | Pattern matches against          | Supports patterns    |
| ----------------------------- | -------------------------------- | -------------------- |
| read, write, edit, glob, grep | File path (relative or absolute) | yes                  |
| bash                          | Command string                   | yes                  |
| task                          | Target agent name                | yes                  |
| webfetch, todoread, todowrite | -                                | no (allow/deny only) |

For file tools, patterns match against the path as given. Absolute paths
start with `/` or a drive letter like `C:/`. Relative paths have no such
prefix and are resolved against the **workspace root**: the git repository
root if one is found, otherwise the current working directory.

| Pattern | Matches                                                   |
| ------- | --------------------------------------------------------- |
| `**`    | Any file at any depth, relative to the workspace root     |
| `*`     | Any file in the workspace root only                       |
| `/**`   | Any file on the system, including other drives on Windows |
| `?`     | Exactly one character in a path segment                   |

```yaml
permission:
  read:
    "**": deny        # catch-all: deny by default
    "src/**": allow   # allow src directory (last match wins)
  grep: allow         # scalar shorthand for { "**": allow }
  task:
    "*": deny         # deny all delegation by default
    "reader-*": allow # allow delegation to reader-* agents (last match wins)
```

!!! note "Rule order matters"

    Rules are evaluated in reverse order: the **last matching rule wins**.<br/>
    Write specific rules **last** in your config so they override the catch-all patterns.

    Common patterns:
    
    - **Default deny, allow specific**: `"**": deny` first, specific `"path/**": allow` last
    - **Default allow, deny specific**: `"**": allow` first, specific `"path/**": deny` last

## Tool overview

| Tool                                 | Core function            | What it does                                            |
| ------------------------------------ | ------------------------ | ------------------------------------------------------- |
| [**read**](#read)                    | `read_file`              | Read a file with offset/limit and optional line numbers |
| [**write**](#write)                  | `write_file`             | Create or overwrite a file at a resolved path           |
| [**edit**](#edit)                    | `edit_file`              | Apply exact text replacements (find-and-replace)        |
| [**glob**](#glob)                    | `glob_files`             | Match filesystem paths by glob pattern                  |
| [**grep**](#grep)                    | `grep_search`            | Search file contents by regex with match metadata       |
| [**bash**](#bash)                    | `execute_command`        | Execute shell commands with timeout and captured output |
| [**webfetch**](#webfetch)            | `fetch_url`              | Fetch a URL and return content as text or markdown      |
| [**todoread**](#todoread-todowrite)  | `read_todos`             | Read shared todo list state                             |
| [**todowrite**](#todoread-todowrite) | `write_todos`            | Update shared todo list state                           |
| [**task**](#task)                    | `TaskInput`/`TaskOutput` | Delegate work to a named sub-agent                      |

### read

Reads a file, optionally with line numbers and a windowed range.

**Parameters:**

| Parameter | Type   | Required | Description                                       |
| --------- | ------ | -------- | ------------------------------------------------- |
| `path`    | string | yes      | Absolute file path (or relative to allowed dirs)  |
| `offset`  | number | no       | Starting line number (1-indexed, default: 1)      |
| `limit`   | number | no       | Max lines to return (default: from tool settings) |

**Output:** Line-numbered file content. Lines beyond `max_line_length` are
truncated with `...`.

**Configurable via [tool settings](#tool-settings):** `line_numbers`, `limit`,
`max_line_length`

### write

Creates or overwrites a file. Creates parent directories if they don't exist.

**Parameters:**

| Parameter | Type   | Required | Description        |
| --------- | ------ | -------- | ------------------ |
| `path`    | string | yes      | File path to write |
| `content` | string | yes      | Content to write   |

**Output:** Confirmation message.

### edit

Applies exact text replacements to a file. The old text must match exactly
(including whitespace and indentation) or the edit fails.

**Parameters:**

| Parameter  | Type   | Required | Description        |
| ---------- | ------ | -------- | ------------------ |
| `path`     | string | yes      | File path to edit  |
| `old_text` | string | yes      | Exact text to find |
| `new_text` | string | yes      | Replacement text   |

**Output:** Confirmation with the number of replacements made.

**Behaviour:**

- If `old_text` matches exactly once, the replacement is applied
- If `old_text` matches multiple times, all occurrences are replaced
- If `old_text` is not found, the edit fails with an error

### glob

Matches filesystem paths by glob pattern. Uses the `.gitignore`-aware `ignore`
crate for fast traversal.

**Parameters:**

| Parameter | Type   | Required | Description                                    |
| --------- | ------ | -------- | ---------------------------------------------- |
| `pattern` | string | yes      | Glob pattern (e.g. `**/*.rs`, `src/**/*.toml`) |
| `path`    | string | no       | Root directory for the search                  |

**Output:** List of matching file paths.

**Configurable via [tool settings](#tool-settings):** `limit`

### grep

Searches file contents by regex pattern. Returns matching lines with metadata.

**Parameters:**

| Parameter | Type   | Required | Description                       |
| --------- | ------ | -------- | --------------------------------- |
| `pattern` | string | yes      | Regex pattern to search for       |
| `path`    | string | no       | File or directory to search in    |
| `include` | string | no       | File pattern filter (e.g. `*.rs`) |

**Output:** Matching lines with line numbers and file paths.

**Configurable via [tool settings](#tool-settings):** `line_numbers`, `limit`,
`max_line_length`

### bash

Executes a shell command with timeout and captured output.

**Parameters:**

| Parameter    | Type   | Required | Description                                     |
| ------------ | ------ | -------- | ----------------------------------------------- |
| `command`    | string | yes      | Shell command to execute                        |
| `timeout_ms` | number | no       | Timeout in milliseconds (default from settings) |
| `workdir`    | string | no       | Working directory for the command               |

**Output:** Combined stdout and stderr. Non-zero exit codes are included in
the output.

**Configurable via [tool settings](#tool-settings):** `timeout_ms`, `max_timeout_ms`

**Sandboxing:** On Linux, you can enable the `linux-bubblewrap` feature to run
commands inside a [bubblewrap] sandbox. See [Sandboxing](sandboxing.md) for details.

### webfetch

Fetches a URL and returns the content. HTML pages are automatically converted
to markdown.

**Parameters:**

| Parameter    | Type   | Required | Description             |
| ------------ | ------ | -------- | ----------------------- |
| `url`        | string | yes      | URL to fetch            |
| `timeout_ms` | number | no       | Timeout in milliseconds |

**Output:** Page content as text or markdown.

**Configurable via [tool settings](#tool-settings):** `timeout_ms`,
`max_timeout_ms`, `max_response_size`

### todoread / todowrite

Shared todo list state for tracking progress across tool calls. Useful for
agents that plan their work in steps.

**todoread** returns the current todo list. **todowrite** validates and
replaces it. Both are stateless between agent runs. Create them with shared
state via [`create_todo_tools`][create_todo_tools] so that reads and writes
refer to the same list.

**todoread parameters:**

*(none - takes no parameters)*

**todoread output:** Current todo list as formatted text.

**todowrite parameters:**

| Parameter | Type  | Required | Description                                                    |
| --------- | ----- | -------- | -------------------------------------------------------------- |
| `todos`   | array | yes      | Complete list of todo items to set (replaces the current list) |

Each todo item:

| Field      | Type   | Required | Values                                             |
| ---------- | ------ | -------- | -------------------------------------------------- |
| `id`       | string | yes      | Stable, non-empty identifier                       |
| `content`  | string | yes      | Short imperative task text                         |
| `status`   | string | yes      | `pending`, `in_progress`, `completed`, `cancelled` |
| `priority` | string | yes      | `high`, `medium`, `low`                            |

**todowrite output:** Confirmation with the number of items set.

### task

The task tool enables multi-agent delegation. An agent can invoke a named
sub-agent with a prompt and receive the result.

**Parameters:**

| Parameter       | Type   | Required | Description                                                  |
| --------------- | ------ | -------- | ------------------------------------------------------------ |
| `subagent_type` | string | yes      | Exact name of the target subagent                            |
| `prompt`        | string | yes      | Full instructions for the delegated agent                    |
| `description`   | string | yes      | Short task label (3-5 words)                                 |
| `command`       | string | no       | Optional context string identifying what triggered this task |

**Output:** The delegated agent's response as a text summary.

See [Getting Started](getting-started.md) for the full delegation model.

## Tool Settings

Some tools expose configurable settings. These are **host-side** constraints,
not parameters the LLM passes per call.

### read

| Setting           | Type    | Default | Min | Description                 |
| ----------------- | ------- | ------- | --- | --------------------------- |
| `line_numbers`    | `bool`  | `true`  | -   | Show line numbers in output |
| `limit`           | `usize` | `2000`  | `1` | Max lines returned per read |
| `max_line_length` | `usize` | `2000`  | `4` | Max characters per line     |

### grep

| Setting           | Type    | Default | Min | Description                   |
| ----------------- | ------- | ------- | --- | ----------------------------- |
| `line_numbers`    | `bool`  | `true`  | -   | Show line numbers in output   |
| `limit`           | `usize` | `100`   | `1` | Max matches returned          |
| `max_line_length` | `usize` | `2000`  | `4` | Max characters per match line |

### glob

| Setting | Type    | Default | Min | Description             |
| ------- | ------- | ------- | --- | ----------------------- |
| `limit` | `usize` | `1000`  | `1` | Max file paths returned |

### bash

| Setting          | Type  | Default  | Min    | Description                     |
| ---------------- | ----- | -------- | ------ | ------------------------------- |
| `timeout_ms`     | `u32` | `120000` | `1000` | Default command timeout (ms)    |
| `max_timeout_ms` | `u32` | `600000` | `1`    | Max timeout the LLM can request |

### webfetch

| Setting             | Type    | Default   | Min    | Description                     |
| ------------------- | ------- | --------- | ------ | ------------------------------- |
| `timeout_ms`        | `u32`   | `30000`   | `1000` | Default fetch timeout (ms)      |
| `max_timeout_ms`    | `u32`   | `600000`  | `1`    | Max timeout the LLM can request |
| `max_response_size` | `usize` | `5242880` | `1`    | Max response body size (bytes)  |

### Setting in agent files

Override defaults in the agent file front matter under `tool_settings`:

```yaml
---
name: my-agent
tool_settings:
  read:
    line_numbers: false
    limit: 500
  bash:
    timeout_ms: 60000
    max_timeout_ms: 300000
---
```

!!! warning "Validation rules"

    - `max_timeout_ms` must be greater than or equal to `timeout_ms` (for both
      bash and webfetch).
    - `max_line_length` must be at least 4 to accommodate the `...`
      truncation suffix plus at least one visible character.

### Override in code

There are two levels of API depending on how you use the library.

**Agent-level settings** (reloaded-code-agents):

Use [`AgentToolSettings`](https://docs.rs/reloaded-code-agents/latest/reloaded_code_agents/struct.AgentToolSettings.html)
when building an agent from an [`AgentConfig`](https://docs.rs/reloaded-code-agents/latest/reloaded_code_agents/struct.AgentConfig.html):

```rust
use reloaded_code_agents::{AgentToolSettings, ReadToolSettings};

let settings = AgentToolSettings {
    read: ReadToolSettings {
        line_numbers: false,
        limit: 500,
        max_line_length: 2000,
    },
    ..AgentToolSettings::default()
};
```

**Tool-level settings** (reloaded-code-core / reloaded-code-serdesai):

Use the builder pattern on each tool when constructing them individually:

```rust
use reloaded_code_core::tools::ReadSettings;
use reloaded_code_serdesai::{ReadTool, AbsolutePathResolver, BashTool};

// Read
let settings = ReadSettings::new()
    .with_default_limit(500)?
    .with_max_line_length(1000)?
    .with_line_numbers(false);
let tool = ReadTool::with_settings(AbsolutePathResolver, settings);

// Bash
let tool = BashTool::new()
    .with_timeouts(Some(30_000), Some(120_000));
```

See the [API reference](https://docs.rs/reloaded-code-core) for the full
builder API on each settings type.

## Path resolvers

File tools (`read`, `write`, `edit`, `glob`, `grep`) are generic over a `PathResolver`.  
This controls which paths the tools can access:

| Resolver               | Behaviour                                                 |
| ---------------------- | --------------------------------------------------------- |
| `AbsolutePathResolver` | Any absolute path is allowed. No restrictions.            |
| `AllowedPathResolver`  | Only paths within configured directories. Sandboxed mode. |
| `AllowedGlobResolver`  | A workspace directory with glob-based allow/deny rules.   |

Agents use `AllowedGlobResolver` by default. If you don't need glob-based rules,
`AllowedPathResolver` or `AbsolutePathResolver` are slightly faster.

For a deeper dive into path security, see [Sandboxing](sandboxing.md).

[bubblewrap]: https://github.com/containers/bubblewrap
[create_todo_tools]: https://docs.rs/reloaded-code-serdesai/latest/reloaded_code_serdesai/tools/todo/fn.create_todo_tools.html
[reloaded-code-core]: https://docs.rs/reloaded-code-core
[reloaded-code-serdesai]: https://docs.rs/reloaded-code-serdesai
[Agents]: agents.md
[agent files]: agents.md
