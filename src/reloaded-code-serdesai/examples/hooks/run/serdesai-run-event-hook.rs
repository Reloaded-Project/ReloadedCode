//! `RunEventHook` interception on a streaming run with a mock model.
//!
//! This example registers two `RunEventHook`s via
//! `AgentRuntimeBuilder::hooks()`, builds an agent with
//! `AgentBuildContext::with_model_override()` using a mock model, and
//! consumes `HookedAgent::run_stream()`. One hook rewrites each text
//! delta to uppercase; the other suppresses the output-ready milestone.
//! The printed stream shows the rewritten text where the mock's raw
//! response would be, and no output-ready line.
//!
//! Mode scoping: `RunEventHook` fires only on `run_stream()`. A
//! registered `RunHook` stays inert on this path, so this example
//! registers none.
//!
//! Expected output:
//!   Built agent with 0 tools.
//!   run started
//!   [UppercaseDeltas] "Mock response" -> "MOCK RESPONSE"
//!   text: "MOCK RESPONSE"
//!   [SuppressOutputReady] dropping output-ready
//!   run complete
//!
//! Run with:
//!   cargo run --example serdesai-run-event-hook -p reloaded-code-serdesai --features mock

use futures::StreamExt;
use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{HookSet, RunEventContext, RunEventHookResult};
use reloaded_code_serdesai::{RunEvent, RunEventHook};

#[path = "../shared.rs"]
mod shared;

/// Suppresses the output-ready milestone; the consumer never sees it.
struct SuppressOutputReady;

/// Rewrites every streamed text delta to uppercase before publication.
struct UppercaseDeltas;

impl RunEventHook for SuppressOutputReady {
    fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
        if matches!(event, RunEvent::OutputReady) {
            println!("[SuppressOutputReady] dropping output-ready");
            return Ok(None);
        }
        Ok(Some(event))
    }
}

impl RunEventHook for UppercaseDeltas {
    fn hook(&self, _ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult {
        match event {
            RunEvent::TextDelta { text } => {
                let upper = text.to_uppercase();
                println!("[UppercaseDeltas] {text:?} -> {upper:?}");
                Ok(Some(RunEvent::TextDelta { text: upper }))
            }
            other => Ok(Some(other)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = HookSet::builder()
        .run_event_hook(UppercaseDeltas)
        .run_event_hook(SuppressOutputReady)
        .build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "event-hook-demo",
        "event hook demo",
        "You are an event hook demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = shared::mock_model();
    let agent = build_context
        .with_model_override(model)
        .build("event-hook-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let mut stream = agent.run_stream("Say hello.", ()).await?;
    while let Some(item) = stream.next().await {
        match item? {
            RunEvent::RunStart { .. } => println!("run started"),
            RunEvent::TextDelta { text } => println!("text: {text:?}"),
            RunEvent::OutputReady => println!("output ready"),
            RunEvent::RunComplete { .. } => println!("run complete"),
            // Step and context telemetry still flows through the hooks;
            // printing it is skipped to keep the output short.
            _ => {}
        }
    }
    Ok(())
}
