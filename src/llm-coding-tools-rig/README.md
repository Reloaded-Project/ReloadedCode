# llm-coding-tools-rig

[![Crates.io](https://img.shields.io/crates/v/llm-coding-tools-rig.svg)](https://crates.io/crates/llm-coding-tools-rig)
[![Docs.rs](https://docs.rs/llm-coding-tools-rig/badge.svg)](https://docs.rs/llm-coding-tools-rig)

Lightweight, high-performance Rig framework Tool implementations for coding tools.

## Features

- **File operations** - Read, write, edit, glob, grep with two access modes:
  - `absolute::*` - Unrestricted filesystem access
  - `allowed::*` - Sandboxed to configured directories
- **Shell execution** - Cross-platform command execution with timeout
- **Web fetching** - URL content retrieval with format conversion
- **Task delegation** - Sub-agent spawning for complex tasks
- **Todo management** - Shared-state todo list tracking
- **Context strings** - LLM guidance text for tool usage (re-exported from core)

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
llm-coding-tools-rig = "0.1"
```

## Quick Start

Run the included example:

```bash
cargo run --example basic -p llm-coding-tools-rig
```

## Usage

### File Operation Tools

File tools (Read, Write, Edit, Glob, Grep) come in two variants:

**`absolute::*`** - Unrestricted filesystem access, requires absolute paths:

```rust
use llm_coding_tools_rig::absolute::{ReadTool, WriteTool, EditTool, GlobTool, GrepTool};

let read = ReadTool::new();   // LINE_NUMBERS defaults to true
let write = WriteTool::new();
let edit = EditTool::new();
let glob = GlobTool::new();
let grep = GrepTool::new();

// Disable line numbers with explicit generic:
let read_raw = ReadTool::<false>::new();
let grep_raw = GrepTool::<false>::new();
```

**`allowed::*`** - Sandboxed to configured directories:

```rust
use llm_coding_tools_rig::allowed::{ReadTool, WriteTool};
use llm_coding_tools_rig::AllowedPathResolver;
use std::path::PathBuf;

// Option 1: Pass paths directly
let read: ReadTool<true> = ReadTool::new([
    PathBuf::from("/home/user/project"),
    PathBuf::from("/tmp/workspace"),
]).unwrap();

// Option 2: Share a resolver across tools (recommended)
let resolver = AllowedPathResolver::new([
    PathBuf::from("/home/user/project"),
]).unwrap();
let read: ReadTool<true> = ReadTool::with_resolver(resolver.clone());
let write = WriteTool::with_resolver(resolver);
```

### Other Tools

Tools that don't operate on files:

```rust
use llm_coding_tools_rig::{BashTool, TaskTool, WebFetchTool, TodoTools};

let bash = BashTool::new();           // Shell command execution
let webfetch = WebFetchTool::new();   // URL content fetching
let task = TaskTool::with_mock();     // Sub-agent delegation
let todos = TodoTools::new();         // Todo list (todos.read, todos.write)
```

### PreambleBuilder

`PreambleBuilder` tracks registered tools and generates a combined context string
for the agent's system prompt. This provides LLM guidance on using each tool effectively.

```rust
use llm_coding_tools_rig::absolute::{ReadTool, GlobTool};
use llm_coding_tools_rig::{BashTool, PreambleBuilder, TodoTools};
use rig::tool::ToolSet;

// Create preamble builder to track tools
let mut pb = PreambleBuilder::new();

// Create todo tools with shared state
let todos = TodoTools::new();

// Build toolset - pb.track() registers each tool and passes it through
let toolset = ToolSet::builder()
    .static_tool(pb.track(ReadTool::<true>::new()))
    .static_tool(pb.track(GlobTool::new()))
    .static_tool(pb.track(BashTool::new()))
    .static_tool(pb.track(todos.read))
    .static_tool(pb.track(todos.write))
    .build();

// Generate preamble with usage instructions for all tracked tools
let preamble = pb.build();

// Use with rig agent:
// let agent = client.agent("gpt-4o")
//     .preamble(&preamble)  // <-- Pass preamble here
//     .tools(toolset)
//     .build();
```

### Context Strings

LLM guidance strings are re-exported from `llm_coding_tools_core`:

```rust
use llm_coding_tools_rig::context::{BASH, READ_ABSOLUTE, READ_ALLOWED};

// Use context strings in system prompts or tool descriptions
println!("{}", BASH);

// Path-based tools have absolute and allowed variants
println!("{}", READ_ABSOLUTE);  // For absolute::ReadTool
println!("{}", READ_ALLOWED);   // For allowed::ReadTool
```

## Examples

```bash
# Basic toolset setup with PreambleBuilder
cargo run --example basic -p llm-coding-tools-rig

# Complete agent configuration (recommended starting point)
cargo run --example full_agent -p llm-coding-tools-rig

# Sandboxed file access with allowed::* tools
cargo run --example sandboxed -p llm-coding-tools-rig
```

## License

Apache 2.0
