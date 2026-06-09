# Hooks

Hooks let your code see or change what an agent does.

## Run hooks

- `RunHook` trait
  - Full control over `RunConfig` (system prompt, preambles, params).
  - Intercept the run, modify config, then call `original` to continue.
  - Skip `original.call()` to block or replace the run entirely.

- `on_run_start` / `on_run_end`
  - Lightweight, no trait boilerplate.
  - `on_run_start` fires before the run begins.
  - `on_run_end` fires after the run completes -- even on error.
  - Cannot modify `RunConfig`; use for logging or metrics.

### Example programs

- serdesai-run-hook
  - Single `RunHook` that injects a system preamble via `RunConfig`.
  - `cargo run --example serdesai-run-hook -p reloaded-code-serdesai --features mock`

- serdesai-run-chain
  - Two `RunHook`s showing nesting order.
  - Registration order A -> B gives execution A-before, B-before, executor, B-after, A-after.
  - `cargo run --example serdesai-run-chain -p reloaded-code-serdesai --features mock`

- serdesai-run-event
  - `on_run_start` and `on_run_end` closures around a run.
  - `cargo run --example serdesai-run-event -p reloaded-code-serdesai --features mock`

### Shared code

`shared.rs` holds common setup: mock `ModelCatalog`, dummy credentials, `build_agent_context`, and `mock_model`. Each example pulls it in with `#[path = "../shared.rs"] mod shared;`. `shared.rs` is not a standalone example.
