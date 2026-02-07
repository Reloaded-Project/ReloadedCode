Basic coding oriented tools for LLM agents.

This is a headless library, there is no TUI interaction model here, so interactive `ask` approval flows and autocomplete-style agent UX are out of scope.

# Feature Flags (llm-coding-tools-core)

- `tokio` (default): Async mode with tokio runtime. Enables async function signatures.
- `blocking`: Sync/blocking mode. Mutually exclusive with `tokio`/`async`.
- `async`: Base async signatures (internal use). Do not enable directly; use `tokio`.

The `async` and `blocking` features are mutually exclusive - enabling both causes a compile error.

# Project Structure

- `llm-coding-tools-core/` - Framework-agnostic core library
- `llm-coding-tools-agents/` - Agent config loading and permission model
- `llm-coding-tools-models-dev/` - models.dev catalog integration and snapshot tooling
- `llm-coding-tools-serdesai/` - serdesAI framework Tool implementations

# Code & Performance Guidelines

This is a high-performance library. Optimize aggressively. Use arrays instead of maps if size is known ahead of time.
Optimize for memory. Preallocate or trim if possible. Minimize memory use. Use smaller integers/types where appropriate. Use any other tricks that improve CPU or memory efficiency.

## Memory & Allocation

- Preallocate collections when size is known or estimable:
  - `String::with_capacity(estimated_len)`
  - `Vec::with_capacity(count)`
  - `BufReader::with_capacity(size, reader)`
- Prefer `&str` / `&[T]` returns over owned types when lifetime allows
- Use `Cow<'_, str>` for conditional ownership (e.g., `String::from_utf8_lossy`)
- Use `&'static str` for compile-time constant strings
- Reuse buffers: `.clear()` and reuse `Vec`/`String` instead of reallocating

## Zero-Cost Abstractions

- Use const generics for compile-time branching (e.g., `<const LINE_NUMBERS: bool>`)
- Use `#[inline]` on small, hot-path functions
- Prefer `core` over `std` where possible (`core::mem` over `std::mem`)

## I/O Efficiency

- Stream data instead of loading entire files when possible
- Use `memchr` for fast byte searching over manual iteration

## Dependencies

- Prefer performance-oriented crates: `parking_lot` over `std::sync`, `memchr` for byte search
- Keep dependency footprint minimal

## General

- Keep modules under 500 lines (excluding tests); split if larger
- Place `use` inside functions only for `#[cfg]` conditional compilation

# Documentation Standards

- Document public items with `///`
- Add examples in docs where helpful
- Use `//!` for module-level docs
- Focus comments on "why" not "what"
- Use [`TypeName`] rustdoc links, not backticks.

# Verification

After code changes or for checks (testing/linting/building/docs/formatting), run `.cargo/verify.sh` (`.cargo/verify.ps1` on Windows). It echoes each command and runs the full suite, including core tests and any extra checks. Do this before returning to the user.
