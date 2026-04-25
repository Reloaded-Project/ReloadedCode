# Comparison with OpenCode

This page breaks down where reloaded-code overlaps with [OpenCode], where it
diverges, and which to pick. [OpenCode] excels at interactive development;
reloaded-code is for embedding agent tools into your own applications.

## At a glance

<div class="md-typeset__table">
<table>
  <thead>
    <tr>
      <th>Aspect</th>
      <th>OpenCode</th>
      <th>reloaded-code</th>
    </tr>
  </thead>
  <tbody>
    <tr><td><strong>What it is</strong></td><td>A coding agent application</td><td>A coding agent library</td></tr>
    <tr><td><strong>Language</strong></td><td>TypeScript</td><td>Rust</td></tr>
    <tr><td><strong>Runtime</strong></td><td><a href="https://bun.sh">Bun</a></td><td>Native binary / library</td></tr>
    <tr><td><strong>Memory</strong></td><td><abbr title="OpenCode v1.14.21&#10;serve: 305 MiB RSS&#10;TUI: 525 MiB RSS&#10;&#10;v1.4.2&#10;serve: 392 MiB RSS&#10;TUI: 679 MiB RSS">~305 MiB RSS (Resident Set Size)</abbr></td><td><abbr title="~13 MiB RSS on release build, all providers enabled.&#10;  • Code &amp; read-only data: ~6.5 MiB&#10;  • Heap (runtime state): ~2.5 MiB&#10;  • Shared libraries (glibc, libm): ~2.3 MiB&#10;  • Thread stacks: ~0.1 MiB (34 threads)&#10;  Private ~2.5 MiB · PSS ~10 MiB.">~13 MiB RSS (Resident Set Size)</abbr></td></tr>
    <tr><td><strong>Interface</strong></td><td>TUI (Terminal User Interface), Desktop, IDE</td><td>Library (no UI - headless)</td></tr>
    <tr><td><strong>Target user</strong></td><td>Developer at a terminal</td><td>Developer building a server/bot/tool</td></tr>
    <tr><td><strong>Agent format</strong></td><td>Markdown + YAML frontmatter</td><td>Similar format</td></tr>
    <tr><td><strong>Permissions</strong></td><td><abbr title="Tools are allowed unless explicitly denied.&#10;An interactive &quot;ask&quot; mode prompts the user for approval in the TUI.">Default-allow + interactive ask</abbr></td><td><abbr title="Tools are denied unless explicitly allowed in agent frontmatter.&#10;No interactive approval - there is no user to prompt.">Default-deny (no interactive mode)</abbr></td></tr>
    <tr><td><strong>Tool set</strong></td><td>14 tools</td><td>10 tools (core set)</td></tr>
    <tr><td><strong>LLM framework</strong></td><td>AI SDK (TypeScript)</td><td><a href="https://crates.io/crates/serdes-ai">SerdesAI</a> / bring your own</td></tr>
    <tr><td><strong>Providers</strong></td><td>75+ via <a href="https://models.dev">models.dev</a></td><td><abbr title="Same models.dev catalog. No Codex or Copilot OAuth adapters yet; PRs welcome.">75+ via <a href="https://models.dev">models.dev</a></abbr></td></tr>
    <tr><td><strong>Sandboxing</strong></td><td>-</td><td>Linux <a href="https://github.com/containers/bubblewrap">bubblewrap</a> (2 profiles)</td></tr>
    <tr><td><strong>Embeddable</strong></td><td><abbr title="OpenCode runs as a separate server process; you embed it by calling its HTTP API from your client. The team is developing additional embedding options at time of writing.">Client/server HTTP API</abbr></td><td>Rust crate (library)</td></tr>
    <tr><td><strong>Async</strong></td><td>Yes (<a href="https://bun.sh">Bun</a>)</td><td>Yes (<a href="https://tokio.rs">tokio</a>) and blocking mode</td></tr>
    <tr><td><strong>System prompt</strong></td><td>~2000+ tokens</td><td>~2000 tokens (dynamically generated, includes only enabled tools)</td></tr>
  </tbody>
</table>
</div>

## Where they overlap

- **Agent markdown format** - both use a similar YAML frontmatter schema
  (`name`, `mode`, `description`, `model`, `permission`, `tool_settings`).
  Agent files written for [OpenCode] are drop-in compatible (add
  explicit permissions). See [Agents](agents.md) for the full format
  reference and [Migrating from OpenCode](migration.md) for the differences.

- **Core tools** - both provide `read`, `write`, `edit`, `glob`, `grep`,
  `bash`, and `webfetch`. See [Tools](tools.md) for the complete tool
  reference.

- **[models.dev]** - both support the models.dev catalog for provider/model
  lookups.

- **Multi-agent delegation** - both support `task` tool delegation with
  recursion depth limits (how deeply agents can delegate to other agents).

## Where they differ

### Permissions

[OpenCode] uses **default-allow**: tools are allowed unless you explicitly
deny them. It also offers an interactive `ask` mode that prompts the user for
approval in the TUI before a tool runs.

reloaded-code uses **default-deny**: every tool is blocked unless you
explicitly allow it in the agent frontmatter. There is no interactive approval
flow because there is no user to prompt - the agent runs unattended.

See [Migrating from OpenCode](migration.md) for a side-by-side YAML example,
a [portable default-deny configuration](migration.md#portable-default-deny),
and a migration checklist.

### Interface

[OpenCode] is a full application with a TUI (Terminal User Interface), desktop
app, VS Code extension, and HTTP API server. reloaded-code is a library
with no UI. You build the interface or API layer yourself.

### Framework

[OpenCode] is built on the [Vercel AI SDK](https://sdk.vercel.ai) (TypeScript). reloaded-code uses
[SerdesAI] for the ready-to-use integration, but the core is
framework-agnostic so you can bring your own LLM framework. See
[Custom Framework Integration](guides/custom-framework.md) for details.

### Sandboxing

[OpenCode] doesn't provide built-in sandboxing. To isolate it, you sandbox the
entire process externally (containers, VMs, etc.). reloaded-code provides
**in-process** sandboxing: each tool is sandboxed individually within your
application.

Two layers are available:

- **Path resolvers** - restrict which paths the file tools
  (`read`, `write`, `edit`, `glob`, `grep`) can access.
  See [Path resolvers](tools.md#path-resolvers) for the resolver types
  and configuration.

- **Shell sandboxing** (Linux only) - sandbox `bash` commands with
  [bubblewrap] using kernel-level filesystem, network, and process
  isolation. Two profiles are available:
  [Public Bot](sandboxing.md#public-bot) (untrusted input) and
  [Trusted Maintenance](sandboxing.md#trusted-maintenance)
  (trusted automation).

Because sandboxing is per-tool, each agent or client can use a different
configuration. See [Sandboxing](sandboxing.md) for the full guide.

### Features unique to OpenCode

- TUI / desktop app / IDE extensions (notifications, themes, keybindings)
- Interactive permission prompts (`ask` mode)
- LSP (Language Server Protocol) integration
- Session sharing (share live agent sessions with other users)

### Features unique to reloaded-code

- In-process sandboxing: path resolvers + shell sandboxing ([bubblewrap])
- Framework-agnostic core (bring your own LLM framework)
- Embeddable inside any process
- Low memory footprint (~10 MiB PSS, all providers enabled)

---

Ready to get started? See [Getting Started](getting-started.md) or
[Migrating from OpenCode](migration.md).

[OpenCode]: https://opencode.ai/
[SerdesAI]: https://crates.io/crates/serdes-ai
[models.dev]: https://models.dev
[bubblewrap]: https://github.com/containers/bubblewrap
[Bun]: https://bun.sh
[tokio]: https://tokio.rs
