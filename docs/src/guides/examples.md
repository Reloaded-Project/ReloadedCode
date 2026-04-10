# Examples

Runnable examples live in the repository under each crate's `examples/` directory.

## SerdesAI Integration

| Example                   | Description                                                                                                           | Run                                                                                                    |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| [serdesai-basic]          | Minimal agent with file tools, shell execution, web fetch, and streaming output.                                      | `cargo run --example serdesai-basic -p llm-coding-tools-serdesai`                                      |
| [serdesai-agents]         | Load markdown agents through `AgentLoader`, build a named agent via `AgentBuildContext` using the models.dev catalog. | `cargo run --example serdesai-agents -p llm-coding-tools-serdesai`                                     |
| [serdesai-task]           | Orchestrator delegates a read-only task to a reader sub-agent, with streamed transcript and tool-call logging.        | `cargo run --example serdesai-task -p llm-coding-tools-serdesai`                                       |
| [serdesai-sandboxed]      | Agent with `AllowedPathResolver` - file operations restricted to specific directories.                                | `cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai`                                  |
| [serdesai-sandboxed-bash] | Sandboxed shell execution with a bubblewrap `public_bot` profile (Linux only).                                        | `cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai` |

[serdesai-basic]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-basic.rs
[serdesai-agents]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-agents.rs
[serdesai-task]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-task.rs
[serdesai-sandboxed]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-sandboxed.rs
[serdesai-sandboxed-bash]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-sandboxed-bash.rs

## Core Library

| Example                          | Description                                                                       | Run                                                                           |
| -------------------------------- | --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| [system_prompt_preview]          | Full system prompt with all tools enabled, prints static token cost breakdown.    | `cargo run --example system_prompt_preview -p llm-coding-tools-core`          |
| [system_prompt_preview_readonly] | Smaller read-only system prompt - minimal tool set, lower token cost.             | `cargo run --example system_prompt_preview_readonly -p llm-coding-tools-core` |
| [system_prompt_preview_compare]  | Compares full vs read-only prompt footprints, prints character and token savings. | `cargo run --example system_prompt_preview_compare -p llm-coding-tools-core`  |

[system_prompt_preview]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-core/examples/system_prompt_preview.rs
[system_prompt_preview_readonly]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-core/examples/system_prompt_preview_readonly.rs
[system_prompt_preview_compare]: https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-core/examples/system_prompt_preview_compare.rs
