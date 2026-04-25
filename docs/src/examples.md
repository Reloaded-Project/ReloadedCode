# Examples

Runnable examples live in the repository under each crate's `examples/` directory.

## SerdesAI Integration

| Example                   | Description                                                                                                           | Run                                                                                                    |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| [serdesai-basic]          | Minimal agent with file tools, shell execution, web fetch, and streaming output.                                      | `cargo run --example serdesai-basic -p reloaded-code-serdesai`                                      |
| [serdesai-agents]         | Load markdown agents through `AgentLoader`, build a named agent via `AgentBuildContext` using the models.dev catalog. | `cargo run --example serdesai-agents -p reloaded-code-serdesai`                                     |
| [serdesai-task]           | Orchestrator delegates a read-only task to a reader sub-agent, with streamed transcript and tool-call logging.        | `cargo run --example serdesai-task -p reloaded-code-serdesai`                                       |
| [serdesai-sandboxed]      | Agent with `AllowedPathResolver` - file operations restricted to specific directories.                                | `cargo run --example serdesai-sandboxed -p reloaded-code-serdesai`                                  |
| [serdesai-sandboxed-bash] | Sandboxed shell execution with a bubblewrap `public_bot` profile (Linux only).                                        | `cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p reloaded-code-serdesai` |

[serdesai-basic]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/examples/serdesai-basic.rs
[serdesai-agents]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/examples/serdesai-agents.rs
[serdesai-task]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/examples/serdesai-task.rs
[serdesai-sandboxed]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/examples/serdesai-sandboxed.rs
[serdesai-sandboxed-bash]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/examples/serdesai-sandboxed-bash.rs

## Core Library

| Example                          | Description                                                                       | Run                                                                           |
| -------------------------------- | --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| [system_prompt_preview]          | Full system prompt with all tools enabled, prints static token cost breakdown.    | `cargo run --example system_prompt_preview -p reloaded-code-core`          |
| [system_prompt_preview_readonly] | Smaller read-only system prompt - minimal tool set, lower token cost.             | `cargo run --example system_prompt_preview_readonly -p reloaded-code-core` |
| [system_prompt_preview_compare]  | Compares full vs read-only prompt footprints, prints character and token savings. | `cargo run --example system_prompt_preview_compare -p reloaded-code-core`  |

[system_prompt_preview]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-core/examples/system_prompt_preview.rs
[system_prompt_preview_readonly]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-core/examples/system_prompt_preview_readonly.rs
[system_prompt_preview_compare]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-core/examples/system_prompt_preview_compare.rs
