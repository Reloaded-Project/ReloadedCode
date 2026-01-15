# llm-coding-tools-core

Lightweight, high-performance core types and utilities for coding tools - framework agnostic.

## Overview

This crate provides the foundational building blocks for coding tool implementations:

- `ToolError` - Unified error type for all tool operations
- `ToolResult<T>` - Result type alias using ToolError
- `ToolOutput` - Wrapper for tool responses with truncation metadata
- Utility functions for text processing and formatting
- `context` module - LLM guidance strings for tool usage

## Features

- `tokio` (default): Async mode with tokio runtime. Enables async function signatures.
- `blocking`: Sync/blocking mode. Mutually exclusive with `tokio`/`async`.
- `async`: Base async signatures (internal). Requires a runtime; use `tokio` instead.

The `async` and `blocking` features are mutually exclusive - enabling both causes a compile error.

Future runtimes (smol, async-std) can be added following the same pattern as `tokio`.

## Usage

```rust
use llm_coding_tools_core::{ToolError, ToolResult, ToolOutput};
use llm_coding_tools_core::util::{truncate_text, format_numbered_line};
```

## Context Module

The `context` module provides embedded strings containing usage guidance for LLM agents.
These can be appended to tool descriptions or system prompts.

Path-based tools have two variants:
- `*_ABSOLUTE`: For unrestricted filesystem access (absolute paths required)
- `*_ALLOWED`: For sandboxed access (paths relative to allowed directories)

```rust
use llm_coding_tools_core::context::{BASH, READ_ABSOLUTE, READ_ALLOWED};

// Non-path tools have a single variant
println!("{}", BASH);

// Path-based tools have absolute and allowed variants
println!("{}", READ_ABSOLUTE);
println!("{}", READ_ALLOWED);
```

Available context strings:
- `BASH`, `TASK`, `TODO_READ`, `TODO_WRITE`, `WEBFETCH` - standalone tools
- `READ_ABSOLUTE`, `READ_ALLOWED` - file reading
- `WRITE_ABSOLUTE`, `WRITE_ALLOWED` - file writing
- `EDIT_ABSOLUTE`, `EDIT_ALLOWED` - file editing
- `GLOB_ABSOLUTE`, `GLOB_ALLOWED` - pattern matching
- `GREP_ABSOLUTE`, `GREP_ALLOWED` - content search

## Design Principles

- No framework-specific dependencies, plug and play into any LLM framework/library
  - See [llm-coding-tools-rig](https://crates.io/crates/llm-coding-tools-rig) for an integration example with [rig](https://crates.io/crates/rig)
- Minimal dependency footprint
- Performance-oriented (optimized) with zero-cost abstractions
