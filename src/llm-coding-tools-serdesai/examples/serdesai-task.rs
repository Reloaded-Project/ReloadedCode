//! Stateless Task delegation example using the models.dev catalog.
//!
//! Loads markdown agents from `examples/agents/`, builds the primary
//! orchestrator through [`AgentRuntimeTaskExt::build_with_task`], and runs one
//! prompt that should delegate exactly once to `reader`.
//!
//! Run: Edit the API_KEY_NAME and API_KEY_VALUE constants below, then:
//!      cargo run --example serdesai-task -p llm-coding-tools-serdesai

use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentDefaults, AgentRuntimeTaskExt};
use std::{path::PathBuf, sync::Arc};

const AGENT_NAME: &str = "orchestrator";
const MODEL_ID: &str = "synthetic/hf:zai-org/GLM-4.7";
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
        .build();

    println!(
        "Loading named agent `{AGENT_NAME}` from {}",
        examples_root.display()
    );
    let agent = runtime.build_with_task(
        AGENT_NAME,
        Arc::new(load_result.catalog),
        Arc::new(credentials),
    )?;
    println!(
        "Built `{AGENT_NAME}` on demand with {} tools.",
        agent.tools().len()
    );

    let prompt = format!(
        "Use a single delegated read-only task to inspect {}. Then explain in three bullets how the task-enabled build flow works, why it lives in SerdesAI, and what v1 Task does not support.",
        readme_path.display(),
    );
    let response = agent.run(prompt.as_str(), ()).await?;
    println!("{}", response.output());

    Ok(())
}
