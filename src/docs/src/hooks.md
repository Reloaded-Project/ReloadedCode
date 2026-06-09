# Hooks

Hooks let your code see, change, or stop things the agent does.

!!! warning "Work in progress"
    Backend wiring is not done yet. Core hooks, run hook types, and container
    exist. [SerdesAI] dispatch code comes next.

Tool hooks work like game mods.
Each hook gets an `original` function.
`original` calls the next hook or the real tool.

This lets you run code before and after the tool call in the same method.

## Example

A hook can modify the request before the tool sees it.

`$HOME` in string arguments expands to the user's home directory:

```rust
use std::env;
use serde_json::Value;
use reloaded_code_core::{
    HookSet, ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal,
    ToolOutput, ToolRequest,
};

struct HomeExpander;

impl ToolHook for HomeExpander {
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        mut req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        Box::pin(async move {
            // Expand $HOME → real home directory in all string args
            if let Some(home) = env::var("HOME").ok() {
                if let Some(map) = req.args.as_object_mut() {
                    for val in map.values_mut() {
                        if let Value::String(s) = val {
                            *s = s.replace("$HOME", &home);
                        }
                    }
                }
            }

            original.call(ctx, req).await
        })
    }
}

let hooks = HookSet::builder()
    .tool_hook(HomeExpander)
    .build();
```

To block or replace a tool call, do not call `original`.

A common case: prevent credential leaks by blocking read/write access
to `.env` files.

```rust
use reloaded_code_core::{
    HookSet, ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal,
    ToolOutput, ToolRequest,
};

struct EnvFileGuard;

impl ToolHook for EnvFileGuard {
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        Box::pin(async move {
            // Block access to .env files
            if let Some(path) = req.args.get("path").and_then(|v| v.as_str()) {
                if path.starts_with(".env") || path.contains("/.env") {
                    return Ok(ToolOutput::new(
                        "Blocked: .env files contain secrets"
                    ));
                }
            }

            // All other calls pass through
            original.call(ctx, req).await
        })
    }
}

let hooks = HookSet::builder()
    .tool_hook(EnvFileGuard)
    .build();
```

## Available types

### Tool hook types

| Type                | Purpose                                                    |
| ------------------- | ---------------------------------------------------------- |
| [`ToolHook`]        | Intercepts a tool call and may call [`ToolOriginal`].      |
| [`ToolOriginal`]    | Pointer to next hook or the real tool.                     |
| [`ToolHookFuture`]  | Boxed future returned by tool hooks.                       |
| [`ToolCallContext`] | Tool name, agent name, run id.                             |
| [`ToolRequest`]     | JSON arguments carried through the hook chain.             |
| [`ToolOutput`]      | Tool call result wrapping content and truncation metadata. |

### Run hook types

| Type              | Purpose                                                      |
| ----------------- | ------------------------------------------------------------ |
| [`RunHook`]       | Intercepts a run and may call [`RunOriginal`].               |
| [`RunOriginal`]   | Pointer to next hook or the real run executor.               |
| [`RunHookFuture`] | Boxed future returned by run hooks.                          |
| [`RunConfig`]     | Mutable config a RunHook can change before calling original. |
| [`RunOutput`]     | Framework-agnostic result of a completed run.                |
| [`RunExecutor`]   | Final callable used at the end of the run hook chain.        |
| [`RunUsage`]      | Token usage for a completed run.                             |

### Container types

| Type               | Purpose                                           |
| ------------------ | ------------------------------------------------- |
| [`HookSet`]        | Stores tool hooks, run hooks, and compact events. |
| [`HookSetBuilder`] | Builder for [`HookSet`].                          |

## How tool hooks stack

This diagram assumes you register two hooks. If you set no hooks, the
tool call goes straight to the real tool (fast path).

Tool hooks run in registration order. Each hook receives an `original`
function. That function calls the next hook or the real tool.

```mermaid
flowchart TD
    TC[Tool call] --> H1[Hook 1]
    H1 -->|"original.call(ctx, req)"| H2[Hook 2]
    H1 -->|"skip original"| Done[Result to agent]
    H2 -->|"original.call(ctx, req)"| Exec[Real tool]
    H2 -->|"skip original"| H1A
    Exec --> H2A[Hook 2 after]
    H2A --> H1A[Hook 1 after]
    H1A --> Done
```
This works like game mods.
Your hook gets a function.
That function is `original`.
It calls the next hook, not always the real tool.

## Getting started

Build a `HookSet` with your hooks, then pass it to the runtime:

```rust
use reloaded_code_agents::AgentRuntimeBuilder;
use reloaded_code_core::HookSet;

let hooks = HookSet::builder()
    .tool_hook(EnvFileGuard)
    .build();

let runtime = AgentRuntimeBuilder::new()
    .hooks(hooks)
    .build()?;

assert!(!runtime.hooks().is_empty());
```

Calling `.hooks(set)` replaces any existing `HookSet`. Omitting it
passes `HookSet::default()`.

## Design notes

- **Everything is a hook**: Functions like `on_run_start` / `on_run_end` 
  are convenience wrappers. They register lightweight hook implementations
  internally. Code before `original` is "start", code after is "end".
  They participate in the same hook chain with the same ordering rules.

- **Natural unwind order.** Hook code after `original.call(...)` runs in
  reverse order. Later hooks run first after the operation.

- **Blocking by omission.** A hook blocks or replaces a call by not calling
  `original`.

- **Empty fast path.** `dispatch_tool` calls the real tool directly when you
  set no hooks.


[`ToolHook`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/trait.ToolHook.html
[`ToolOriginal`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.ToolOriginal.html
[`ToolHookFuture`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/type.ToolHookFuture.html
[`ToolCallContext`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.ToolCallContext.html
[`ToolRequest`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.ToolRequest.html
[`ToolOutput`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.ToolOutput.html
[`HookSet`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.HookSet.html
[`HookSetBuilder`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.HookSetBuilder.html
[`RunHook`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/trait.RunHook.html
[`RunOriginal`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.RunOriginal.html
[`RunHookFuture`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/type.RunHookFuture.html
[`RunConfig`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.RunConfig.html
[`RunOutput`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.RunOutput.html
[`RunExecutor`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/trait.RunExecutor.html
[`RunUsage`]: https://docs.rs/reloaded-code-core/latest/reloaded_code_core/struct.RunUsage.html
[SerdesAI]: https://crates.io/crates/serdes-ai
