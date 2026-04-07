//! Markdown-agent runtime example using models.dev catalog.
//!
//! Loads markdown agents through [`AgentLoader`], builds one named agent through
//! [`AgentBuildContext`], and runs it.
//!
//! The model catalog is loaded from models.dev, which provides up-to-date
//! provider and model information from <https://models.dev/api.json>.
//!
//! Run: Edit the API_KEY_NAME and API_KEY_VALUE constants below, then:
//!      cargo run --example serdesai-agents -p llm-coding-tools-serdesai

use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentBuildContext, AgentDefaults};
use std::{path::PathBuf, sync::Arc};

const AGENT_NAME: &str = "basic/file-reader";
const MODEL_ID: &str = "synthetic/hf:zai-org/GLM-4.7-Flash";
const API_KEY_NAME: &str = "SYNTHETIC_API_KEY";
const API_KEY_VALUE: &str = ""; // <-- Set your API key here

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
    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(credentials),
    );

    println!(
        "Loading named agent `{AGENT_NAME}` from {}",
        examples_root.display()
    );
    let agent = build_context.build(AGENT_NAME)?;
    println!(
        "Built `{AGENT_NAME}` on demand with {} tools.",
        agent.tools().len()
    );

    let prompt = format!(
        "Read {} and summarize the runtime flow in three bullets.",
        readme_path.display()
    );
    let response = agent.run(prompt.as_str(), ()).await?;
    println!("{}", response.output());

    Ok(())
}
