//! Event-style hooks with a real SerdesAI agent and mock model.
//!
//! Uses `on_run_start` and `on_run_end` closures registered via
//! `AgentRuntimeBuilder::hooks()`, builds a SerdesAI agent with a mock
//! model override, and verifies the callbacks fire around the run.
//!
//! Expected output:
//!   [on_run_start] agent=demo-agent
//!   [on_run_end] agent=demo-agent, reason=Completed
//!   Output: Hello from the mock model.
//!
//! Run with:
//!   cargo run --example serdesai-run-event -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{EndReason, HookRunContext, HookSet};

#[path = "../shared.rs"]
mod shared;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = HookSet::builder()
        .on_run_start(|ctx: &HookRunContext<'_>| {
            println!("[on_run_start] agent={}", ctx.agent_name);
        })
        .on_run_end(|ctx: &HookRunContext<'_>, reason: EndReason| {
            println!("[on_run_end] agent={}, reason={:?}", ctx.agent_name, reason);
        })
        .build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "event-demo",
        "event demo",
        "You are an event demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = shared::mock_model();
    let agent = build_context
        .with_model_override(model)
        .build("event-demo")?;

    let response = agent.run("Say hello.", ()).await?;
    println!("Output: {}", response.output());
    Ok(())
}
