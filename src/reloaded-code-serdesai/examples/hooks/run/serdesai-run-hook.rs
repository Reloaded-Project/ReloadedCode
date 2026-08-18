//! `RunHook` lifecycle demo with a read-only run config view and mock model.
//!
//! `RunHook` controls the run lifecycle on `run()` only and observes the
//! final run config read-only; config mutation belongs to `RunConfigHook`.
//! This example registers one hook of each kind via
//! `AgentRuntimeBuilder::hooks()`: the config hook injects a preamble, and
//! the run hook reads that resolved config without mutating it. The run
//! hook observes the finished output on the first run, then skips the
//! second run and returns a synthetic reply instead of calling the
//! original run.
//!
//! Expected output:
//!   Built agent with 0 tools.
//!   [PreambleInjector] injecting preamble for agent=lifecycle-demo
//!   [LifecycleHook] resolved preamble: You are a helpful assistant.
//!   [LifecycleHook] run finished: Mock response
//!   Output: Mock response
//!   [PreambleInjector] injecting preamble for agent=lifecycle-demo
//!   [LifecycleHook] skipping the run
//!   Output: synthetic reply
//!
//! Run with:
//!   cargo run --example serdesai-run-hook -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    EndReason, HookRunContext, HookSet, PreambleMessage, PreambleRole, RunConfig, RunConfigHook,
    RunConfigHookFuture, RunHook, RunHookFuture, RunOriginal, RunOutput, RunUsage,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[path = "../shared.rs"]
mod shared;

/// Run hook that observes the first run and skips every later one.
struct LifecycleHook {
    observed_a_run: AtomicBool,
}

struct PreambleInjector;

impl RunHook for LifecycleHook {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: &'a RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            if self.observed_a_run.swap(true, Ordering::SeqCst) {
                println!("[LifecycleHook] skipping the run");
                return Ok(RunOutput {
                    content: "synthetic reply".into(),
                    reason: EndReason::Completed,
                    usage: RunUsage::default(),
                });
            }

            // Read-only view: the config hook already amended this config.
            let preamble = config
                .preamble_messages
                .first()
                .map(|message| message.content.as_str())
                .unwrap_or("<none>");
            println!("[LifecycleHook] resolved preamble: {preamble}");

            let output = original.call(ctx).await?;
            println!("[LifecycleHook] run finished: {}", output.content);
            Ok(output)
        })
    }
}

impl RunConfigHook for PreambleInjector {
    fn configure<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        config: &'a mut RunConfig,
    ) -> RunConfigHookFuture<'a> {
        Box::pin(async move {
            println!(
                "[PreambleInjector] injecting preamble for agent={}",
                ctx.agent_name
            );
            config.preamble_messages.push(PreambleMessage {
                role: PreambleRole::System,
                content: "You are a helpful assistant.".into(),
            });
            Ok(())
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hooks = HookSet::builder()
        .run_config_hook(PreambleInjector)
        .run_hook(LifecycleHook {
            observed_a_run: AtomicBool::new(false),
        })
        .build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "lifecycle-demo",
        "lifecycle demo",
        "You are a lifecycle demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = shared::mock_model();
    let agent = build_context
        .with_model_override(model)
        .build("lifecycle-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let response = agent.run("Say hello.", ()).await?;
    println!("Output: {}", response.output());

    let response = agent.run("Say hello again.", ()).await?;
    println!("Output: {}", response.output());
    Ok(())
}
