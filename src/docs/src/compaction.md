# Compaction

Long conversations grow until they no longer fit the model's context
window. Compaction summarizes the older history through the run's own
model, keeping the run inside its input limit instead of failing.

Compaction is opt-in and configured at agent build time.

## Enable compaction

Call `with_compaction` on the `AgentBuildContext` with a
`CompactPolicy`:

```rust
use reloaded_code_core::CompactPolicy;
use reloaded_code_serdesai::AgentBuildContext;

let build_context = AgentBuildContext::new(
    runtime,
    model_catalog,
    credentials,
    workspace_root,
)
.with_compaction(CompactPolicy::default());
```

Every agent built from the context checks each model request against
the policy's trigger threshold. Past the threshold, older history is
summarized before the model sees the request.

A build that skips `with_compaction` keeps compaction disabled: no
model wrapper, no per-request estimation, no compaction events.

`with_compaction` must run before the context is shared. It panics
when the `AgentBuildContext` has already been cloned.

## Defaults

`CompactPolicy::default()` behaves as follows:

| Setting                | Default | Meaning                                                                                                      |
| ---------------------- | ------- | ------------------------------------------------------------------------------------------------------------ |
| `trigger_margin`       | 32,000  | Compaction triggers once a request's estimated tokens reach the context limit minus this margin.             |
| `trigger_fraction`     | 3/4     | Context limits at or below the margin trigger at this fraction of the limit, so small windows still compact. |
| `summarize_max_output` | 32,000  | Output-token cap of the summarize request, clamped to the model's advertised maximum output.                 |

A 200,000-token window triggers at 168,000 estimated tokens. A
32,000-token window triggers at 24,000, which is 3/4 of the window.

Token counts are estimates: serialized request bytes divided by four.
The margin absorbs the estimate's error; a provider may still reject a
request the estimate undercounts.

Compaction needs the model's input limit. Models resolved through the
catalog carry it; see [Models Catalog]. Without a known limit,
compaction never triggers.

## Override the policy

Start from `CompactPolicy::default()` and set the fields that differ:

```rust
use reloaded_code_core::CompactPolicy;

let policy = CompactPolicy {
    // Trigger 8,000 tokens below the context limit.
    trigger_margin: 8_000,
    ..CompactPolicy::default()
};
```

`trigger_fraction` takes a `CompactFraction`. It governs the
small-window case where the margin does not fit:

```rust
use reloaded_code_core::{CompactFraction, CompactPolicy};

let policy = CompactPolicy {
    // Small windows trigger at half the context limit.
    trigger_fraction: CompactFraction::new(1, 2),
    // Cap the summarize request at 16,000 output tokens.
    summarize_max_output: 16_000,
    ..CompactPolicy::default()
};
```

## What a compaction does

When a request crosses the threshold:

1. Leading system messages stay verbatim.
2. Older messages are summarized through the run's own model, which
   keeps its sampling settings.
3. Up to the four most recent messages stay verbatim. A tool result
   is never split from the call it answers, so the window can start
   later and keep fewer.
4. The summary lands as one system message ahead of the kept window.

The summarize request caps its output tokens at the policy value,
clamped to the model's advertised maximum output. Its prompt asks for
the longest, most detailed summary the budget allows, so detail is
lost only when the budget forces it.

Summaries are memoized: a later compaction covering the same older
messages reuses the stored summary without a new request.

## Failure never stops the run

Compaction fails open. Any error, from estimation, a failed summarize
call, or an empty summary, aborts the attempt: the original history is
served unchanged, no event publishes, and the run continues.

## Observing compaction

`run_stream()` publishes one `RunEvent::ContextCompressed` per applied
compaction:

```rust
use reloaded_code_serdesai::RunEvent;

match event {
    RunEvent::ContextCompressed {
        original_tokens,
        compressed_tokens,
        strategy,
        messages_before,
        messages_after,
    } => println!(
        "context compressed: {original_tokens} -> {compressed_tokens} tokens, \
         strategy {strategy:?}, {messages_before} -> {messages_after} messages"
    ),
    _ => {}
}
```

Skipped and failed compactions publish no event. `run()` applies
compaction without events; only `run_stream()` exposes them.

Full example: [serdesai-compact]
(`cargo run --example serdesai-compact -p reloaded-code-serdesai`).

[Models Catalog]: models-catalog.md
[serdesai-compact]: https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/examples/serdesai-compact.rs
