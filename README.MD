# llm-coding-tools

[![Crates.io - llm-coding-tools-core](https://img.shields.io/crates/v/llm-coding-tools-core.svg)](https://crates.io/crates/llm-coding-tools-core)
[![Crates.io - llm-coding-tools-serdesai](https://img.shields.io/crates/v/llm-coding-tools-serdesai.svg)](https://crates.io/crates/llm-coding-tools-serdesai)
[![CI](https://github.com/Sewer56/llm-coding-tools/actions/workflows/rust.yml/badge.svg)](https://github.com/Sewer56/llm-coding-tools/actions/workflows/rust.yml)

Lightweight, high-performance coding tool implementations for LLM-powered development agents.

## About This Workspace

This workspace contains multiple Rust crates for integrating coding tools with LLM agents:

- **[llm-coding-tools-core](./src/llm-coding-tools-core/)**: Framework-agnostic core operations and utilities
- **[llm-coding-tools-serdesai](./src/llm-coding-tools-serdesai/)**: serdesAI framework-specific Tool implementations
- **[llm-coding-tools-agents](./src/llm-coding-tools-agents/)**: Agent configuration loading for registry-driven Task tools

## Features

- **File Operations**: Read, write, edit files with line-numbered output
- **Search**: Glob pattern matching and regex content search  
- **Shell**: Cross-platform command execution with timeout
- **Web**: URL fetching with HTML-to-markdown conversion
- **Path Security**: Choose between unrestricted or sandboxed file access
- **Context Strings**: Embedded LLM guidance for tool usage

## Feature Flags (llm-coding-tools-core)

- `tokio` (default): Async mode with tokio runtime
- `blocking`: Sync/blocking mode, mutually exclusive with `async`

## Quick Start

See [llm-coding-tools-serdesai](./src/llm-coding-tools-serdesai/README.md) for serdesAI framework support.

## Examples

```bash
# serdesAI framework - Basic agent setup
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# serdesAI framework - Sandboxed file access
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# serdesAI framework - Registry-driven agent invocation
cargo run --example serdesai-agents -p llm-coding-tools-serdesai
```

## Documentation

- [llm-coding-tools-core README](./src/llm-coding-tools-core/README.md)
- [llm-coding-tools-serdesai README](./src/llm-coding-tools-serdesai/README.md)
- [llm-coding-tools-agents README](./src/llm-coding-tools-agents/README.md)
- [Developer Guidelines](./src/AGENTS.md)

## Contributing

Contributions are welcome! Please ensure all tests pass and the code follows our guidelines.

## License

Licensed under [Apache 2.0](./LICENSE).
