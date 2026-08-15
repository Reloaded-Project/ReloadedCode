//! Multiple `ToolHook`s auditing and hardening one real `bash` call.
//!
//! This example registers two `ToolHook`s around a single real `bash`
//! execution: the outer audit hook prints the original arguments and the
//! run context, and the inner hardening hook, registered via
//! `shared_tool_hook` so it sits directly on the real tool, clones the
//! [`ToolRequest`], injects a `timeout_ms` into the arguments, and sends
//! the hardened clone on. The fixed `echo` command writes nothing, so the
//! example performs no file I/O and the only subprocess is that one
//! command. Permission rules can allow or deny a `bash` call, but they
//! cannot log it or rewrite its arguments; only hooks can.
//!
//! The injected timeout never fires: the command finishes long before it,
//! so the hardening is observable through the printed hardened arguments
//! plus the one successful execution, not through a timeout.
//!
//! Expected output:
//!   Built agent with 1 tools.
//!   [AuditHook] tool=bash agent=tool-chain-demo run_id=<unique per run> args={"command":"echo reloaded tool hooks"}
//!   [HardeningHook] original args={"command":"echo reloaded tool hooks"}
//!   [HardeningHook] hardened args={"command":"echo reloaded tool hooks","timeout_ms":5000}
//!   Output: Tool call finished.
//!
//!   reloaded tool hooks
//!
//! Run with:
//!   cargo run --example serdesai-tool-chain -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    HookSet, ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest,
};
use reloaded_code_serdesai::mock::tool_then_text;
use serde_json::json;
use std::sync::Arc;

#[path = "../shared.rs"]
mod shared;

/// Fixed trivial command the scripted model runs; it writes nothing.
const COMMAND: &str = "echo reloaded tool hooks";
/// stdout of [`COMMAND`]; only a real bash execution can put it in the
/// model-facing transcript.
const EXPECTED_ECHO: &str = "reloaded tool hooks";
/// Timeout the hardening hook injects, in milliseconds.
const HARDENED_TIMEOUT_MS: u32 = 5_000;

struct AuditHook;

struct HardeningHook;

impl ToolHook for AuditHook {
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        Box::pin(async move {
            println!(
                "[AuditHook] tool={} agent={} run_id={} args={}",
                ctx.tool_name, ctx.agent_name, ctx.run_id, req.args
            );
            original.call(ctx, req).await
        })
    }
}

impl ToolHook for HardeningHook {
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        Box::pin(async move {
            // Clone so the original arguments stay intact for the log line;
            // only the hardened clone reaches the real tool.
            let mut hardened = req.clone();
            if let Some(args) = hardened.args.as_object_mut() {
                args.insert("timeout_ms".into(), json!(HARDENED_TIMEOUT_MS));
            }
            println!("[HardeningHook] original args={}", req.args);
            println!("[HardeningHook] hardened args={}", hardened.args);
            original.call(ctx, hardened).await
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Registration order is outer-to-inner: the audit hook sees the model's
    // original arguments, the hardening hook transforms them right before
    // the real bash tool.
    let hardening: Arc<dyn ToolHook> = Arc::new(HardeningHook);
    let hooks = HookSet::builder()
        .tool_hook(AuditHook)
        .shared_tool_hook(hardening)
        .build();

    let catalog = AgentCatalog::from_entries([shared::agent_config_with_tools(
        "tool-chain-demo",
        "tool chain demo",
        "You are a tool chain demo agent.",
        &["bash"],
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let model = tool_then_text("bash", json!({"command": COMMAND}), "Tool call finished.");
    let agent = build_context
        .with_model_override(model)
        .build("tool-chain-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let response = agent.run("Run the demo command.", ()).await?;
    // Only a real bash execution can echo this into the transcript, so the
    // assert fails the run if the hardened call never reached the tool.
    assert!(
        response.output().contains(EXPECTED_ECHO),
        "the hardened bash call should have executed exactly once"
    );
    println!("Output: {}", response.output());
    Ok(())
}
