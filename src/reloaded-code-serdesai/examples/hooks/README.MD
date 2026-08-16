# Hooks

Hooks let your code see or change what an agent does.

## Run hooks

- `RunHook` trait
  - Full control over `RunConfig` (system prompt, preambles, params).
  - Intercept the run, modify config, then call `original` to continue.
  - Skip `original.call()` to block or replace the run entirely.
  - Observe start and end: code before `original` is "start", code
    after is "end" (logging, metrics).

### Example programs

- serdesai-run-hook
  - Single `RunHook` that injects a system preamble via `RunConfig`.
  - `cargo run --example serdesai-run-hook -p reloaded-code-serdesai --features mock`

- serdesai-run-chain
  - Two `RunHook`s showing nesting order.
  - Registration order A -> B gives execution A-before, B-before, executor, B-after, A-after.
  - `cargo run --example serdesai-run-chain -p reloaded-code-serdesai --features mock`

## Tool hooks

- `ToolHook` trait
  - Full control over tool arguments and results.
  - Inspect or rewrite arguments, call `original` to run the real tool,
    then wrap the result.
  - Skip `original.call()` to block the call and return your own response.

### Example programs

- serdesai-tool-hook
  - Single `ToolHook` that lets a real `read` of a secrets fixture run,
    then scrubs the `API_KEY=` and `TOKEN=` values from the result
    before the model sees it.
  - `cargo run --example serdesai-tool-hook -p reloaded-code-serdesai --features mock`

- serdesai-tool-block
  - Stateful `ToolHook` that records each file the run reads, then skips
    `original` on a `write` to a never-read file; the real `write` never
    executes and the hook-supplied response completes the run.
  - `cargo run --example serdesai-tool-block -p reloaded-code-serdesai --features mock`

- serdesai-tool-chain
  - Two `ToolHook`s around one real `bash` call: an audit hook logs the
    original arguments and run context, and a hardening hook injects a
    `timeout_ms` into the arguments before the real tool runs; the
    second is registered via `shared_tool_hook`.
  - `cargo run --example serdesai-tool-chain -p reloaded-code-serdesai --features mock`

## Shared code

`shared.rs` holds common setup: mock `ModelCatalog`, dummy credentials, `build_agent_context`, `mock_model`, and agent config fixtures. Each example pulls it in with `#[path = "../shared.rs"] mod shared;`. `shared.rs` is not a standalone example.

Run examples build their agent with `agent_config`; tool examples use `agent_config_with_tools`, whose permission rules allow the named standard tools so the agent has real tools to intercept.
