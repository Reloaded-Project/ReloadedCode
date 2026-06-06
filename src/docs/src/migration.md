# Migrating from OpenCode

reloaded-code uses an agent file format that mirrors [OpenCode]'s schema.
Many agent files are drop-in compatible, but not identical. This page covers
the differences and how to adapt your existing agent files.

## Default-deny permissions

The most visible difference: in [OpenCode], unlisted tools are allowed by
default. In reloaded-code, unlisted tools are **denied**.

=== "OpenCode (default-allow)"

    All tools are allowed; list only the ones you want to deny:

    ```yaml
    permission:
      task: deny       # deny specific tools
      webfetch: deny   # everything else is allowed by default
    ```

=== "reloaded-code (default-deny)"

    All tools are denied; list only the ones you want to allow:

    ```yaml
    permission:
      read: allow      # allow specific tools
      write: allow
      bash: allow
      glob: allow
      grep: allow
      edit: allow
      # everything else is denied by default
    ```

One lists **exceptions** to deny; the other lists **exceptions** to allow.
The two configs are not equivalent: OpenCode has more tools available
overall.

### Portable default-deny

!!! tip "One config, both systems"

    A `"*": deny` catch-all overrides OpenCode's default-allow, so the
    same agent file behaves as default-deny on both systems. In
    reloaded-code it is redundant (the default is already deny), but
    harmless.

```yaml
permission:
  "*": deny         # catch-all: deny every tool
  read: allow       # then allow only what's needed
  write: allow
  bash: allow
  glob: allow
  grep: allow
  edit: allow
  webfetch: allow
```

**The last-match-wins rule** means the system evaluates entries in reverse
order - named `allow` entries that follow `"*": deny` take precedence.
For path-level and command-level patterns (`**`, `*` within a tool), see
[Permission rules](tools.md#permission-rules).

!!! tip "Why portability matters"

    The author uses [OpenCode] on the desktop for interactive development
    and reloaded-code for server-side deployments. reloaded-code
    preserves portability with [OpenCode]'s agent format where possible,
    so you can share agent files across both workflows.

## File tool path matching

!!! info "[OpenCode] has an `external_directory` permission for granting access to paths outside the workspace."

reloaded-code does not have this field: external directories are
specified directly in per-tool filters, which also gives finer-grained
control (each tool can have its own set of paths).

File tool permissions (`read`, `write`, `edit`, `glob`, `grep`) match
against the path as given:

- **Absolute paths** start with `/` or a drive letter like `C:/`
- **Relative paths** have no such prefix

| Pattern | Matches                                                   |
| ------- | --------------------------------------------------------- |
| `**`    | Any file at any depth, relative to the workspace root     |
| `*`     | Any file in the workspace root only                       |
| `/**`   | Any file on the system, including other drives on Windows |

```yaml
permission:
  read:
    "**": deny                        # deny everything else
    "src/**": allow                   # relative: src directory
    "/var/data/config.yaml": allow    # absolute: specific file
  write:
    "**": deny
    "src/**": allow
```

## No interactive `ask` mode

[OpenCode] supports `permission.task: ask` which prompts the user for approval
in the TUI. reloaded-code is headless, so there is no interactive approval
flow.

Using `ask` will produce a schema validation error. Use `allow` or `deny`
instead:

```yaml
permission:
  task:
    "*": deny           # deny all delegation by default
    "reader-*": allow   # allow delegation to reader-* agents
```

!!! note "Omitting `permission.task` defaults to allow"

    When `permission.task` is omitted entirely, it defaults to allowing
    delegation to all callable subagents. This is an exception to the
    default-deny rule. To disable delegation, explicitly set `task: deny`.

## Quick migration checklist

- [ ] Add explicit `permission` entries for every tool the agent needs
- [ ] Replace `external_directory` with per-tool path filter patterns
- [ ] Change `ask` to `allow` or `deny` in all permission rules
- [ ] Verify path patterns match expected files
- [ ] Test with `cargo test` or your integration tests

[OpenCode]: https://opencode.ai/
