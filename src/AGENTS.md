Basic coding oriented tools for LLM agents

# Feature Flags (llm-coding-tools-core)

- `tokio` (default): Async mode with tokio runtime. Enables async function signatures.
- `blocking`: Sync/blocking mode. Mutually exclusive with `tokio`/`async`.
- `async`: Base async signatures (internal use). Do not enable directly; use `tokio`.

The `async` and `blocking` features are mutually exclusive - enabling both causes a compile error.

# Project Structure

- `llm-coding-tools-core/` - Framework-agnostic core library
  - `src/operations/` - Core operation implementations (read, write, edit, glob, grep, bash, etc.)
  - `src/path/` - Path resolution (absolute and allowed)
  - `src/error.rs` - Unified error types
  - `src/output.rs` - Tool output formatting
  - `src/util.rs` - Shared utilities
- `llm-coding-tools-rig/` - Rig framework Tool implementations
  - `src/absolute/` - Unrestricted file system tools
  - `src/allowed/` - Sandboxed file system tools
  - `src/bash.rs`, `src/task.rs`, etc. - Standalone tools

# Code & Performance Guidelines

This is a high-performance library. Optimize aggressively.

## Memory & Allocation

- Preallocate collections when size is known or estimable:
  - `String::with_capacity(estimated_len)`
  - `Vec::with_capacity(count)`
  - `BufReader::with_capacity(size, reader)`
- Use power-of-two sizes for allocator efficiency: `.next_power_of_two()`
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

# Post-Change Verification

All must pass without warnings:

```bash
cargo build -p llm-coding-tools-core && cargo build -p llm-coding-tools-rig --quiet && cargo test -p llm-coding-tools-core && cargo test -p llm-coding-tools-rig --quiet && cargo clippy -p llm-coding-tools-core -- -D warnings && cargo clippy -p llm-coding-tools-rig --quiet -- -D warnings && cargo test -p llm-coding-tools-core --no-default-features --features blocking --quiet && cargo doc --workspace --no-deps --quiet && cargo fmt --all
```

Note: `llm-coding-tools-rig` is async-only (implements rig's async `Tool` trait).
The `blocking` feature only applies to `llm-coding-tools-core`.

For individual crates:
```bash
cargo publish --dry-run -p llm-coding-tools-core --quiet && cargo publish --dry-run -p llm-coding-tools-rig --quiet
```
