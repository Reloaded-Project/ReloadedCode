//! Single `ToolHook` scrubbing secret values out of a real `read` result.
//!
//! This example registers a `ToolHook` via `AgentRuntimeBuilder::hooks()`,
//! builds an agent whose permission rules allow the `read` standard tool,
//! and scripts the mock model with `mock::tool_then_text` so the first
//! model turn reads `service.env`, a fixture containing `API_KEY=` and
//! `TOKEN=` lines. The real `read` executes inside a temp workspace, the
//! hook rewrites the tool's result, replacing the secret values with
//! `[REDACTED]` before the result returns to the model, so the final
//! output shows the file with secrets scrubbed. A permission rule can
//! allow or deny the read, but it cannot rewrite the result; only a hook
//! can.
//!
//! Expected output:
//!   Built agent with 1 tools.
//!   [SecretRedactor] scrubbed 2 secret values from tool=read of service.env
//!   Output: Tool call finished.
//!
//!      1: # Demo service configuration.
//!      2: API_KEY=[REDACTED]
//!      3: TOKEN=[REDACTED]
//!      4: LOG_LEVEL=debug
//!
//! Run with:
//!   cargo run --example serdesai-tool-hook -p reloaded-code-serdesai --features mock

use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    HookSet, ToolCallContext, ToolHook, ToolHookFuture, ToolOriginal, ToolRequest,
};
use reloaded_code_serdesai::mock::tool_then_text;
use serde_json::json;

#[path = "../shared.rs"]
mod shared;

/// Value prefixes of the fixture's secret lines; the final transcript
/// check uses them to prove the raw values never reach the model.
const RAW_SECRET_PREFIXES: [&str; 2] = ["sk-demo-", "tok-demo-"];
/// Replacement written in place of each scrubbed secret value.
const REDACTED: &str = "[REDACTED]";
/// Workspace fixture the scripted model reads.
const SECRETS_FILE: &str = "service.env";
/// Assignment keys whose values get scrubbed from tool results.
///
/// Deliberately example-local: a fixed key list, not a secret catalog or a
/// general scanning policy.
const SECRET_KEYS: [&str; 2] = ["API_KEY", "TOKEN"];

struct SecretRedactor;

impl ToolHook for SecretRedactor {
    fn hook<'a>(
        &'a self,
        ctx: &'a ToolCallContext<'a>,
        req: ToolRequest,
        original: ToolOriginal<'a>,
    ) -> ToolHookFuture<'a> {
        Box::pin(async move {
            let mut output = original.call(ctx, req).await?;
            let (scrubbed, count) = scrub_secrets(&output.content);
            // The fixture has exactly one assignment line per secret key, so
            // any other count means the scrub or the fixture drifted.
            assert_eq!(
                count,
                SECRET_KEYS.len(),
                "expected to scrub one value per secret key"
            );
            println!(
                "[SecretRedactor] scrubbed {count} secret values from tool={} of {SECRETS_FILE}",
                ctx.tool_name
            );
            output.content = scrubbed;
            Ok(output)
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = shared::temp_workspace();
    let hooks = HookSet::builder().tool_hook(SecretRedactor).build();

    let catalog = AgentCatalog::from_entries([shared::agent_config_with_tools(
        "tool-hook-demo",
        "tool hook demo",
        "You are a tool hook demo agent.",
        &["read"],
    )]);

    let build_context =
        shared::build_agent_context_in_workspace(catalog, hooks, workspace.root.clone());

    let model = tool_then_text(
        "read",
        json!({"file_path": SECRETS_FILE}),
        "Tool call finished.",
    );
    let agent = build_context
        .with_model_override(model)
        .build("tool-hook-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let response = agent.run("Read the service configuration.", ()).await?;
    // The scrubbed values must reach the model, not just be computed: a raw
    // fixture prefix in the transcript means propagation dropped the rewrite.
    for prefix in RAW_SECRET_PREFIXES {
        assert!(
            !response.output().contains(prefix),
            "the model-facing transcript must not contain the raw secret value {prefix:?}"
        );
    }
    println!("Output: {}", response.output());
    Ok(())
}

/// Replaces the values of `KEY=`-assignment lines with `[REDACTED]`.
///
/// Returns the scrubbed text plus the number of values replaced. Lines that
/// assign to none of [`SECRET_KEYS`] pass through unchanged, which is why the
/// benign `LOG_LEVEL` line survives in the printed output.
fn scrub_secrets(content: &str) -> (String, usize) {
    let mut count = 0;
    let mut scrubbed = String::with_capacity(content.len());
    for line in content.lines() {
        match scrub_line(line) {
            Some(redacted) => {
                count += 1;
                scrubbed.push_str(&redacted);
            }
            None => scrubbed.push_str(line),
        }
        scrubbed.push('\n');
    }
    // `lines()` drops the shape of the final newline; restore it so the
    // scrubbed result keeps the original content's exact tail.
    if !content.ends_with('\n') {
        scrubbed.pop();
    }
    (scrubbed, count)
}

/// Scrubs one line when it assigns to a secret key, else returns `None`.
///
/// Tolerates the read tool's `{number}: ` line prefix so numbered output
/// stays aligned with the file's line numbers.
fn scrub_line(line: &str) -> Option<String> {
    let (prefix, body) = split_number_prefix(line);
    let key = SECRET_KEYS
        .iter()
        .find(|key| body.starts_with(*key) && body[key.len()..].starts_with('='))?;
    Some(format!("{prefix}{key}={REDACTED}"))
}

/// Splits a leading, possibly padded `{number}: ` prefix off a line.
///
/// The read tool pads line numbers to a fixed width, so the digits can be
/// preceded by spaces. Returns empty-string prefix when the line has no
/// such prefix.
fn split_number_prefix(line: &str) -> (&str, &str) {
    let padding = line.len() - line.trim_start().len();
    let rest = &line[padding..];
    if let Some(index) = rest.find(": ") {
        let digits = &rest[..index];
        if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return line.split_at(padding + index + 2);
        }
    }
    ("", line)
}
