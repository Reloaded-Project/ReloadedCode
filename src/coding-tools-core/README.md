# coding-tools-core

Core types and utilities for coding tools - framework agnostic.

## Overview

This crate provides the foundational building blocks for coding tool implementations:

- `ToolError` - Unified error type for all tool operations
- `ToolResult<T>` - Result type alias using ToolError
- `ToolOutput` - Wrapper for tool responses with truncation metadata
- Utility functions for text processing and formatting

## Usage

```rust
use coding_tools_core::{ToolError, ToolResult, ToolOutput};
use coding_tools_core::util::{truncate_text, format_numbered_line};
```

## Design Principles

- No framework-specific dependencies, plug and play into any LLM framework/library
    - See [coding-tools-rig](https://crates.io/crates/coding-tools-rig) for an integration example with [rig](https://crates.io/crates/rig)
- Minimal dependency footprint
- Performance-oriented (optimized) with zero-cost abstractions
