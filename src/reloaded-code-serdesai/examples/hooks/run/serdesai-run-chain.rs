//! Multiple `RunHook`s with a real SerdesAI agent and mock model.
//!
//! Registers two hooks via `AgentRuntimeBuilder::hooks()` and demonstrates
//! the expected nesting order: A-before -> B-before -> Executor -> B-after -> A-after.
//!
//! Expected output:
//!   [FirstHook] before
//!   [SecondHook] before
//!   [SecondHook] after
//!   [FirstHook] after
//!   Output: Mock response
//!
//! Run with:
//!   cargo run --example serdesai-run-chain -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{HookRunContext, HookSet, RunConfig, RunHook, RunHookFuture, RunOriginal};

#[path = "../shared.rs"]
mod shared;

struct FirstHook;

struct SecondHook;

impl RunHook for FirstHook {
    fn hook<'a>(
        &'a self,
        _ctx: &'a HookRunContext<'a>,
        _config: &'a RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!("[FirstHook] before");
            let output = original.call(_ctx).await?;
            println!("[FirstHook] after");
            Ok(output)
        })
    }
}

impl RunHook for SecondHook {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        _config: &'a RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!("[SecondHook] before");
            let output = original.call(ctx).await?;
            println!("[SecondHook] after");
            Ok(output)
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = HookSet::builder()
        .run_hook(FirstHook)
        .run_hook(SecondHook)
        .build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "chain-demo",
        "chain demo",
        "You are a chain demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = shared::mock_model();
    let agent = build_context
        .with_model_override(model)
        .build("chain-demo")?;

    let response = agent.run("Say hello.", ()).await?;
    println!("Output: {}", response.output());
    Ok(())
}
