//! Context compaction example - tightening the default policy's trigger margin.
//!
//! Builds the `basic/file-reader` markdown agent through
//! [`AgentBuildContext`] with context compaction enabled under a
//! tightened trigger margin, then consumes `run_stream()` and prints
//! the `RunEvent::ContextCompressed` event each applied compaction
//! publishes.
//!
//! The model catalog comes from models.dev, so the resolved model
//! carries a real context window and the threshold can trigger. The
//! prompt grows the transcript with repeated file reads; every
//! over-threshold request compacts the older history.
//!
//! Run: Edit the API_KEY_NAME and API_KEY_VALUE constants below, then:
//!      cargo run --example serdesai-compact -p reloaded-code-serdesai

use futures::StreamExt;
use reloaded_code_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use reloaded_code_core::{CompactPolicy, CredentialResolver, resolve_workspace_root};
use reloaded_code_models_dev::ModelsDevCatalog;
use reloaded_code_serdesai::{AgentBuildContext, AgentDefaults, RunEvent};
use serdes_ai::UserContent;
use std::{path::PathBuf, sync::Arc};

const AGENT_NAME: &str = "basic/file-reader";
const API_KEY_NAME: &str = "SYNTHETIC_API_KEY";
const API_KEY_VALUE: &str = ""; // <-- Set your API key here
const MODEL_ID: &str = "synthetic/hf:zai-org/GLM-4.7-Flash";
/// Compaction trigger margin, in tokens. Tightens the 32,000 default
/// so tool-heavy conversations compact sooner.
const TRIGGER_MARGIN: u32 = 8_000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let examples_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let readme_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    let mut credentials = CredentialResolver::without_env();
    if !API_KEY_VALUE.is_empty() {
        credentials.set_override(API_KEY_NAME, API_KEY_VALUE);
    }

    // Load model catalog from models.dev (online-first with local cache fallback)
    let load_result = ModelsDevCatalog::load().await?;
    println!(
        "Loaded model catalog from models.dev (source: {:?})",
        load_result.source
    );

    let mut catalog = AgentCatalog::new();
    AgentLoader::new().add_directory(&mut catalog, &examples_root)?;

    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog)
        .defaults(AgentDefaults::with_model(MODEL_ID))
        .build()?;

    // Compaction runs by default; this policy overrides only the
    // trigger margin. Summarize cap and small-window fraction keep
    // their 32,000 and 3/4 defaults.
    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(credentials),
        Arc::from(resolve_workspace_root()?),
    )
    .with_compaction(CompactPolicy {
        trigger_margin: TRIGGER_MARGIN,
        ..CompactPolicy::default()
    });

    let agent = build_context.build(AGENT_NAME)?;
    println!("Built `{AGENT_NAME}` with {} tools.", agent.tools().len());

    let prompt = format!(
        "Read {readme}, then re-read it and quote each section heading with its first line. \
         Repeat the full read three times, reporting a running heading total after each pass.",
        readme = readme_path.display(),
    );

    println!("\n=== Compacted stream (compaction events only) ===");
    let mut compactions = 0u32;
    let mut stream = agent.run_stream(UserContent::text(prompt), ()).await?;
    while let Some(event) = stream.next().await {
        match event? {
            // Unfortunately this is non-deterministic so pretend this works for now, ok?
            RunEvent::ContextCompressed {
                original_tokens,
                compressed_tokens,
                strategy,
                messages_before,
                messages_after,
            } => {
                compactions = compactions.saturating_add(1);
                println!(
                    "context compressed: {original_tokens} -> {compressed_tokens} tokens, \
                     strategy {strategy:?}, {messages_before} -> {messages_after} messages"
                );
            }
            RunEvent::RunComplete { .. } => println!("run complete"),
            _ => {}
        }
    }

    println!("{compactions} compaction(s) applied");
    Ok(())
}
