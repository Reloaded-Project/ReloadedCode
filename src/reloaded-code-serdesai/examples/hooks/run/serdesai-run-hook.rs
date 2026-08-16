//! Single `RunHook` with a real SerdesAI agent and mock model.
//!
//! This example registers a `RunHook` via `AgentRuntimeBuilder::hooks()`,
//! builds an agent with `AgentBuildContext::with_model_override()` using a
//! mock model, and runs it. The hook injects a system preamble via
//! `RunConfig` and prints a confirmation message.
//!
//! Expected output:
//!   Built agent with 0 tools.
//!   [PreambleInjector] injecting preamble for agent=hook-demo
//!   Output: Mock response
//!
//! Run with:
//!   cargo run --example serdesai-run-hook -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    HookRunContext, HookSet, PreambleMessage, PreambleRole, RunConfig, RunHook, RunHookFuture,
    RunOriginal,
};

#[path = "../shared.rs"]
mod shared;

struct PreambleInjector;

impl RunHook for PreambleInjector {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        mut config: RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!(
                "[PreambleInjector] injecting preamble for agent={}",
                ctx.agent_name
            );
            config.preamble_messages.push(PreambleMessage {
                role: PreambleRole::System,
                content: "You are a helpful assistant.".into(),
            });
            original.call(ctx, config).await
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = HookSet::builder().run_hook(PreambleInjector).build();

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
