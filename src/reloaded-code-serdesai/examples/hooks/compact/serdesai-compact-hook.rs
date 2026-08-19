//! `CompactHook` interception on a streaming run with a mock model.
//!
//! This example registers a `CompactHook` via
//! `AgentRuntimeBuilder::hooks()`, builds an agent with
//! `AgentBuildContext::with_model_override()` using a mock model whose
//! small context window the prompt overflows, and consumes
//! `HookedAgent::run_stream()`.
//!
//! The hook skips `original`, so the default summarization never runs.
//! It applies a local compaction instead: keep the leading system
//! entries, replace everything after with one summary note, and return
//! its own `CompactResult`. The printed stream shows the hook firing at
//! the overflowing step and the `ContextCompressed` event reporting the
//! hook's strategy and counts. Returning `CompactOutcome::Cancelled`
//! instead would keep the history unchanged and emit no event.
//!
//! Expected output:
//!   Built agent with 0 tools.
//!   prompt of 6821 chars against a 1024-token window
//!   [LocalCompactor] 2 history entries at the overflowing step
//!   [LocalCompactor] local summary: 1 entries replaced by one note
//!   context compressed: 1716 -> 28 tokens, strategy "local", 2 -> 2 messages
//!   run complete
//!
//! Run with:
//!   cargo run --example serdesai-compact-hook -p reloaded-code-serdesai --features mock

use futures::StreamExt;
use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    CompactHook, CompactHookFuture, CompactMessage, CompactOriginal, CompactOutcome, CompactResult,
    HookRunContext, HookSet, RunMessageRole,
};
use reloaded_code_serdesai::RunEvent;
use reloaded_code_serdesai::mock::{FunctionModel, Streamed};
use serdes_ai::core::ModelResponse;
use serdes_ai_models::ModelProfile;

#[path = "../shared.rs"]
mod shared;

/// Context window of the mock model's profile, in tokens. Compaction
/// dispatches once a request's estimated tokens pass three quarters of
/// the window.
const CONTEXT_WINDOW: u64 = 1024;

/// Compact hook that skips `original` and compacts locally: keep the
/// leading system entries, replace everything after with one summary
/// note.
struct LocalCompactor;

impl CompactHook for LocalCompactor {
    fn hook<'a>(
        &'a self,
        _ctx: &'a HookRunContext<'a>,
        mut history: Vec<CompactMessage>,
        _original: CompactOriginal<'a>,
    ) -> CompactHookFuture<'a> {
        Box::pin(async move {
            println!(
                "[LocalCompactor] {} history entries at the overflowing step",
                history.len()
            );
            let tokens_before = estimated_tokens(&history);
            let messages_before = history.len();
            // The split leaves the leading system entries in `history`
            // and moves everything after into `replaced`.
            let system_count = history
                .iter()
                .take_while(|entry| entry.role() == RunMessageRole::System)
                .count();
            let replaced = history.split_off(system_count);
            let summary = format!("{} entries replaced by one note", replaced.len());
            history.push(CompactMessage::new(
                RunMessageRole::System,
                format!("Summary of the earlier conversation:\n{summary}"),
            ));
            println!("[LocalCompactor] local summary: {summary}");
            let result = CompactResult {
                summary,
                first_kept_entry_id: None,
                tokens_before,
                tokens_after: estimated_tokens(&history),
                strategy: "local".to_owned(),
                messages_before,
                messages_after: history.len(),
            };
            Ok((CompactOutcome::Compacted(result), history))
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prompt = format!("Summarize this log.\n\n{}", "server log line. ".repeat(400));

    let hooks = HookSet::builder().compact_hook(LocalCompactor).build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "compact-hook-demo",
        "compact hook demo",
        "You are a compaction demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = Streamed::new(
        FunctionModel::new(|_messages, _settings| {
            ModelResponse::text("continuing from the compacted history")
        })
        .with_profile(ModelProfile::new().with_context_window(CONTEXT_WINDOW)),
    );
    let agent = build_context
        .with_model_override(model)
        .build("compact-hook-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());
    println!(
        "prompt of {} chars against a {CONTEXT_WINDOW}-token window",
        prompt.len()
    );

    let mut stream = agent.run_stream(prompt, ()).await?;
    while let Some(item) = stream.next().await {
        match item? {
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
            RunEvent::RunComplete { .. } => println!("run complete"),
            _ => {}
        }
    }
    Ok(())
}

/// Estimated tokens of a history view: text bytes over four.
fn estimated_tokens(history: &[CompactMessage]) -> usize {
    history
        .iter()
        .map(|entry| entry.text().len())
        .sum::<usize>()
        / 4
}
