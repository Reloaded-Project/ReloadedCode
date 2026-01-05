# rig-coding-tools

Basic coding tools for rig based LLM agents

# Feature Flags (coding-tools-core)

- `tokio` (default): Async mode with tokio runtime. Enables async function signatures.
- `blocking`: Sync/blocking mode. Mutually exclusive with `tokio`/`async`.
- `async`: Base async signatures (internal use). Do not enable directly; use `tokio`.

The `async` and `blocking` features are mutually exclusive - enabling both causes a compile error.

# Project Structure

- `coding-tools-core/` - Framework-agnostic core library
  - `src/operations/` - Core operation implementations (read, write, edit, glob, grep, bash, etc.)
  - `src/path/` - Path resolution (absolute and allowed)
  - `src/error.rs` - Unified error types
  - `src/output.rs` - Tool output formatting
  - `src/util.rs` - Shared utilities
- `coding-tools-rig/` - Rig framework Tool implementations
   - `src/absolute/` - Unrestricted file system tools
   - `src/allowed/` - Sandboxed file system tools
   - `src/bash.rs`, `src/task.rs`, etc. - Standalone tools

# Code Guidelines

- Optimize for performance; use zero-cost abstractions, avoid allocations.
- Keep modules under 500 lines (excluding tests); split if larger.
- Place `use` inside functions only for `#[cfg]` conditional compilation.
- Prefer `core` over `std` where possible (`core::mem` over `std::mem`).

# Documentation Standards

- Document public items with `///`
- Add examples in docs where helpful
- Use `//!` for module-level docs
- Focus comments on "why" not "what"
- Use [`TypeName`] rustdoc links, not backticks.

# Post-Change Verification

All must pass without warnings:

```bash
# Test async mode (default)
cargo build -p coding-tools-core && cargo build -p coding-tools-rig --quiet
cargo test -p coding-tools-core && cargo test -p coding-tools-rig --quiet
cargo clippy -p coding-tools-core && cargo clippy -p coding-tools-rig --quiet -- -D warnings

# Test blocking mode (coding-tools-core only, rig is inherently async)
cargo test -p coding-tools-core --no-default-features --features blocking --quiet

cargo doc --workspace --no-deps --quiet
cargo fmt --all --quiet
```

Note: `coding-tools-rig` is async-only (implements rig's async `Tool` trait).
The `blocking` feature only applies to `coding-tools-core`.

For individual crates:
```bash
cargo publish --dry-run -p coding-tools-core --quiet
cargo publish --dry-run -p coding-tools-rig --quiet
```
