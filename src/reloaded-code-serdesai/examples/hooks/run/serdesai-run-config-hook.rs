//! `RunConfigHook` preamble injection on both run paths with a mock model.
//!
//! This example registers a `RunConfigHook` via `AgentRuntimeBuilder::hooks()`
//! that injects a preamble message into every run's config, then runs the
//! same agent through `HookedAgent::run()` and `HookedAgent::run_stream()`.
//!
//! Mode scoping: `RunConfigHook` fires on `run()` and `run_stream()`.
//! `RunHook` keeps lifecycle control on `run()` only; run the
//! `serdesai-run-hook` example for that demo.
//!
//! Expected output:
//!   Built agent with 0 tools.
//!   [PreambleInjector] injecting preamble for agent=config-hook-demo
//!   run() prompt:
//!   [System] You are a helpful assistant.
//!
//!   Say hello.
//!   [PreambleInjector] injecting preamble for agent=config-hook-demo
//!   run_stream() prompt:
//!   [System] You are a helpful assistant.
//!
//!   Say hello.
//!
//! Run with:
//!   cargo run --example serdesai-run-config-hook -p reloaded-code-serdesai --features mock

use futures::StreamExt;
use reloaded_code_agents::AgentCatalog;
use reloaded_code_core::{
    HookRunContext, HookSet, PreambleMessage, PreambleRole, RunConfig, RunConfigHook,
    RunConfigHookFuture,
};
use reloaded_code_serdesai::RunEvent;
use reloaded_code_serdesai::mock::{FunctionModel, Streamed};
use serdes_ai::core::{ModelResponse, UserContent, UserContentPart};

#[path = "../shared.rs"]
mod shared;

struct PreambleInjector;

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
    let hooks = HookSet::builder().run_config_hook(PreambleInjector).build();

    let catalog = AgentCatalog::from_entries([shared::agent_config(
        "config-hook-demo",
        "config hook demo",
        "You are a config hook demo agent.",
    )]);

    let build_context = shared::build_agent_context(catalog, hooks);

    let agent = build_context
        .with_model_override(echo_model())
        .build("config-hook-demo")?;
    println!("Built agent with {} tools.", agent.tools().len());

    let response = agent.run("Say hello.", ()).await?;
    println!("run() prompt:\n{}", response.output());

    let mut stream = agent.run_stream("Say hello.", ()).await?;
    let mut streamed_prompt = String::new();
    while let Some(item) = stream.next().await {
        if let RunEvent::TextDelta { text } = item? {
            streamed_prompt.push_str(&text);
        }
    }
    println!("run_stream() prompt:\n{streamed_prompt}");
    Ok(())
}

/// Returns a mock model that echoes the last user prompt as its response.
fn echo_model() -> Streamed<FunctionModel> {
    Streamed::new(FunctionModel::new(|messages, _settings| {
        let prompt = messages
            .iter()
            .rev()
            .flat_map(|message| message.user_prompts())
            .next()
            .map(|part| render_prompt(&part.content))
            .unwrap_or_default();
        ModelResponse::text(prompt)
    }))
}

/// Renders a user prompt the way a text-only model would read it.
///
/// The streaming path delivers the prompt as text parts with the injected
/// section head first, so text parts join with a blank line to rebuild the
/// text the non-streaming path receives in one piece.
fn render_prompt(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part {
                UserContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}
