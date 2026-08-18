//! Single `RunHook` with a real SerdesAI agent and mock model.
//!
//! This example registers a `RunHook` via `AgentRuntimeBuilder::hooks()`,
//! builds an agent with `AgentBuildContext::with_model_override()` using a
//! mock model, and runs it. The hook prints a confirmation message.
//!
//! Expected output:
//!   Built agent with 0 tools.
//!   [RunLogger] run starting for agent=hook-demo
//!   Output: Mock response
//!
//! Run with:
//!   cargo run --example serdesai-run-hook -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{HookRunContext, HookSet, RunConfig, RunHook, RunHookFuture, RunOriginal};

#[path = "../shared.rs"]
mod shared;

struct RunLogger;

impl RunHook for RunLogger {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        _config: &'a RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!("[RunLogger] run starting for agent={}", ctx.agent_name);
            original.call(ctx).await
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = HookSet::builder().run_hook(RunLogger).build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "hook-demo",
        "demo agent",
        "You are a demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = shared::mock_model();
    let agent = build_context
        .with_model_override(model)
        .build("hook-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let response = agent.run("Say hello.", ()).await?;
    println!("Output: {}", response.output());
    Ok(())
}
