//! `ToolHook` denies a `write` to a file the run never read.
//!
//! - Turn 1: real `read` of `service.env`.
//! - Turn 2: `write` to `draft.md`, never read.
//! - Hook tracks read files in a `Mutex` set; denies unseen writes without
//!   calling [`ToolOriginal`], so the file never lands on disk.
//! - Paths compared raw; real deployments would canonicalize.
//! - Permission rules cannot depend on earlier calls; stateful hooks can.
//!
//! Expected output:
//!   Built agent with 2 tools.
//!   [ReadBeforeWrite] read recorded: service.env
//!   [ReadBeforeWrite] denying write to never-read file: draft.md
//!   [ReadBeforeWrite] the real write was not executed
//!   [ReadBeforeWrite] draft.md exists after the run: false
//!   Output: Run finished.
//!
//!      1: # Demo service configuration.
//!      2: API_KEY=sk-demo-9f41c2a7b8e04d65
//!      3: TOKEN=tok-demo-6b83d15a92f0
//!      4: LOG_LEVEL=debug
//!   [blocked by hook] write to draft.md denied: no read of that file happened this run
//!
//! Run with:
//!   cargo run --example serdesai-tool-block -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    HookSet, ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal, ToolOutput, ToolRequest,
};
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

#[path = "../shared.rs"]
mod shared;

/// Workspace fixture the scripted model reads first.
const READ_SOURCE: &str = "service.env";
/// Write target the scripted model never reads first.
const WRITE_TARGET: &str = "draft.md";

struct ReadBeforeWrite {
    /// Files the run has read. Interior mutability because the shared hook
    /// instance fires for every tool call, `read` and `write` alike.
    read_files: Mutex<HashSet<PathBuf>>,
}

impl ToolHook for ReadBeforeWrite {
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        Box::pin(async move {
            let target = req
                .args
                .get("file_path")
                .and_then(|value| value.as_str())
                .map(PathBuf::from);

            match (ctx.tool_name, target) {
                ("read", Some(path)) => {
                    // Record every read attempt, successful or not, before
                    // releasing the lock; nothing is held across the await.
                    self.read_files
                        .lock()
                        .expect("read_files should not be poisoned")
                        .insert(path.clone());
                    println!("[ReadBeforeWrite] read recorded: {}", path.display());
                    original.call(ctx, req).await
                }
                ("write", Some(path)) => {
                    let was_read = self
                        .read_files
                        .lock()
                        .expect("read_files should not be poisoned")
                        .contains(&path);
                    if was_read {
                        return original.call(ctx, req).await;
                    }
                    println!(
                        "[ReadBeforeWrite] denying write to never-read file: {}",
                        path.display()
                    );
                    // Skipping `original` is what blocks the call: the real
                    // `write` sits behind it and is never reached.
                    println!("[ReadBeforeWrite] the real write was not executed");
                    Ok(ToolOutput::new(format!(
                        "[blocked by hook] write to {} denied: no read of that file happened this run",
                        path.display()
                    )))
                }
                _ => original.call(ctx, req).await,
            }
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = shared::temp_workspace();
    let hook = ReadBeforeWrite {
        read_files: Mutex::new(HashSet::new()),
    };
    let hooks = HookSet::builder().tool_hook(hook).build();

    let catalog = AgentCatalog::from_entries([shared::agent_config_with_tools(
        "tool-block-demo",
        "tool block demo",
        "You are a tool block demo agent.",
        &["read", "write"],
    )]);

    let build_context =
        shared::build_agent_context_in_workspace(catalog, hooks, workspace.root.clone());

    let model = shared::two_tools_then_text(
        ("read", json!({"file_path": READ_SOURCE})),
        (
            "write",
            json!({"file_path": WRITE_TARGET, "content": "Draft notes."}),
        ),
        "Run finished.",
    );
    let agent = build_context
        .with_model_override(model)
        .build("tool-block-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let response = agent
        .run("Read the config, then write the draft.", ())
        .await?;

    // The denial must be observable on disk, not just in the transcript.
    assert!(
        !workspace.unread_target.exists(),
        "the denied write must not create {}",
        workspace.unread_target.display()
    );
    println!("[ReadBeforeWrite] {WRITE_TARGET} exists after the run: false");
    println!("Output: {}", response.output());
    Ok(())
}
