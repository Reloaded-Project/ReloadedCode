# llm-coding-tools-bubblewrap

Builds bubblewrap profiles, availability checks, and wrapped commands for `llm-coding-tools`.

**Linux only.**

## Main Types

- [`Builder`] - Builds a bubblewrap profile.
- [`Profile`] - A validated bubblewrap profile ready for reuse.
- [`Availability::detect`] - Checks whether `bwrap` can run.
- [`wrap::wrap_command`] - Builds a `bwrap` command from a profile.
- `tokio::build_command_wrap` - Builds the async wrapped command.
- `blocking::build_command_wrap` - Builds the blocking wrapped command.

## Feature Flags

- `tokio`: enables `tokio::build_command_wrap`.
- `blocking`: enables `blocking::build_command_wrap`.

## Usage

### Building a Profile

```rust,no_run
use llm_coding_tools_bubblewrap::{
    Preset, Builder, TmpBacking,
};
use std::path::Path;

fn main() -> Result<(), llm_coding_tools_bubblewrap::LinuxBwrapError> {
let profile = Builder::public_bot(
    Path::new("/host/workspace"),         // workspace: host directory mounted into the sandbox
    Path::new("/tmp/sandbox-home"),       // synthetic_home: host dir mounted as $HOME (/home/sandbox) inside the sandbox
    Path::new("/tmp/sandbox-cache"),      // cache_root: host cache root used for sandbox cache/state dirs
    Some(TmpBacking::Tmpfs),              // tmp_backing: how sandbox /tmp is backed (RAM or host dir)
)
.build()?;

assert_eq!(profile.preset(), Some(Preset::PublicBot));
    Ok(())
}
```

### Detecting Availability

```rust,no_run
use llm_coding_tools_bubblewrap::Availability;

match Availability::detect() {
    Availability::Available => {
        println!("sandbox is ready");
    }
    Availability::Unavailable { reason } => {
        eprintln!("sandbox unavailable: {reason}");
    }
    Availability::Unknown => {
        println!("availability not checked");
    }
}
```

### Wrapping a Command

```rust,no_run
use llm_coding_tools_bubblewrap::{
    wrap, Preset, Builder, TmpBacking,
};
use std::path::Path;

fn main() -> Result<(), llm_coding_tools_bubblewrap::LinuxBwrapError> {
let profile = Builder::public_bot(
    Path::new("/host/workspace"),         // workspace: host directory mounted into the sandbox
    Path::new("/tmp/sandbox-home"),       // synthetic_home: host dir mounted as $HOME (/home/sandbox) inside the sandbox
    Path::new("/tmp/sandbox-cache"),      // cache_root: host cache root used for sandbox cache/state dirs
    Some(TmpBacking::Tmpfs),              // tmp_backing: how sandbox /tmp is backed (RAM or host dir)
)
.build()?;

let wrapped = wrap::wrap_command(
    &profile,                  // profile: validated profile from Builder::build()
    "echo hello",              // command: shell command string to execute
    None,                      // workdir: host working directory (None = use workspace)
).unwrap();
assert!(wrapped.program().ends_with("bwrap"));
    Ok(())
}
```

### Running with Tokio

```text
// tokio::build_command_wrap(&profile, command, workdir)
//   profile: validated Profile
//   command: shell command string to execute
//   workdir: host working directory (None = use workspace)
```
See `tokio::build_command_wrap` (requires `tokio` feature).

### Running with Blocking

```text
// blocking::build_command_wrap(&profile, command, workdir)
//   profile: validated Profile
//   command: shell command string to execute
//   workdir: host working directory (None = use workspace)
```
See `blocking::build_command_wrap` (requires `blocking` feature).

## Presets

- [`Preset::PublicBot`] - Safer defaults for untrusted input. Uses a
  synthetic home, a cleaned `PATH`, read-only system mounts, optional RAM-backed
  `/tmp`, and no network.
- [`Preset::TrustedMaintenance`] - Broader defaults for trusted jobs.
  Uses a read-only host root, a cleaned `PATH`, writable overlays, host-backed
  `/tmp`, and keeps network on.

`TrustedMaintenance` is only for trusted jobs. A command can send out any data
it can read.

Preset helpers return a builder, so you can still change paths, mounts, and env
vars before calling `.build()`. That build step validates profile-owned inputs
and precomputes the reusable `bwrap` argv prefix.

[`TmpBacking::Tmpfs`] keeps sandbox `/tmp` in memory. Use
[`TmpBacking::BindHost`] to mount a host directory at `/tmp`.

[`wrap::wrap_command`] tries a visible host `bash` first and falls back to `sh`.
On Nix systems that is often under `/nix/store/...`. On FHS systems it is often
under `/usr/bin` or `/bin`.

[`Preset::PublicBot`] filters out user-home, temp, wrapper, and
per-user profile directories from the inherited `PATH`.
[`Preset::TrustedMaintenance`] keeps more host `PATH` entries, but
still drops entries under directories hidden by the profile.

For more details on sandbox profiles and trade-offs, see
[SANDBOX-PROFILES.md](https://github.com/Sewer56/llm-coding-tools/blob/main/SANDBOX-PROFILES.md).

## Builder Lists

Setters like `with_read_only_mounts` replace the whole list. They do not append.
That keeps the builder state easy to read.

## Errors

- Missing `bwrap` is reported clearly.
- Environments that cannot create a sandbox are reported clearly.
- Invalid profile-owned paths and invalid credential mounts are rejected at build time.
- Invalid per-command working directories are rejected before spawn.

For the internal architecture and module layout, see [ARCHITECTURE.md](ARCHITECTURE.md).

[`Availability::detect`]: crate::Availability::detect
[`Profile`]: crate::Profile
[`Builder`]: crate::Builder
[`wrap::wrap_command`]: crate::wrap::wrap_command
