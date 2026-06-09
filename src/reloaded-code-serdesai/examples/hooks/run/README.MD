# Run Hooks — Examples

Three approaches to hook into agent runs:

| File | Approach | Use When |
|---|---|---|
| `run-hook.rs` | `RunHook` trait — full control over config + async interception | Need to modify config, inspect/modify output, or conditionally skip the run |
| `chain.rs` | Multiple `RunHook`s — nesting order and unwind behavior | Need multiple independent hook plugins that compose |
| `event-hooks.rs` | `on_run_start` / `on_run_end` callbacks — lightweight, no trait | Just need to observe (logging, metrics) without altering config or output |

## RunHook (run-hook.rs)

Implement `RunHook` for a struct. The `hook()` method receives:
- `ctx` — run context (agent name, run id, model)
- `config` — mutable `RunConfig` (system prompt, preambles, params)
- `original` — call to proceed to the next hook or the executor

You **must** call `original.call(ctx, config).await` to continue the chain.

## Chaining (chain.rs)

Register multiple `RunHook`s via `HookSet::builder().run_hook(A).run_hook(B)`.  
They nest: code before `original.call()` executes in registration order, code after executes in reverse (unwind).

Registration order: **A → B**  
Execution order: **A-before → B-before → [executor] → B-after → A-after**

## Event Hooks (event-hooks.rs)

Use `on_run_start` (fire before run) and `on_run_end` (fire after run, even on error) closures.  
Simpler API than `RunHook` — no trait, no async, no config modification.

## Running

```bash
# RunHook trait example
cargo run --example hooks-run-hook -p reloaded-code-serdesai --features mock

# Chaining example
cargo run --example hooks-run-chain -p reloaded-code-serdesai --features mock

# Event hooks example
cargo run --example hooks-run-event -p reloaded-code-serdesai --features mock
```
