use reloaded_code_agents::{
    AgentCatalog, AgentConfig, AgentDefaults, AgentMode, AgentRuntimeBuilder,
};
use reloaded_code_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
    ProviderSource, ProviderType,
};
use reloaded_code_core::{CredentialResolver, HookSet, resolve_workspace_root};
use reloaded_code_serdesai::AgentBuildContext;
use reloaded_code_serdesai::mock::Streamed;
use serdes_ai_models::MockModel;
use std::path::Path;
use std::sync::Arc;

const DEFAULT_MODEL_ID: &str = "openrouter/cutie/patootie";

/// Builds an `AgentConfig` fixture.
///
/// # Arguments
///
/// - `name` - the agent name.
/// - `description` - the agent description.
/// - `prompt` - the agent system prompt.
pub fn agent_config(name: &str, description: &str, prompt: &str) -> AgentConfig {
    AgentConfig {
        name: name.into(),
        mode: AgentMode::Primary,
        description: description.into(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission: Default::default(),
        options: Default::default(),
        tool_settings: Default::default(),
        prompt: prompt.into(),
    }
}

/// Builds an `AgentBuildContext` wired to mock models and credentials.
///
/// # Arguments
///
/// - `catalog` - the agent catalog to attach to the runtime.
/// - `hooks` - the hook set to install on the runtime.
pub fn build_agent_context(catalog: AgentCatalog, hooks: HookSet) -> AgentBuildContext {
    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog)
        .defaults(AgentDefaults::with_model(DEFAULT_MODEL_ID))
        .hooks(hooks)
        .build()
        .expect("runtime should build");

    AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(model_catalog()),
        mock_credentials(),
        workspace_root(),
    )
}

/// Returns a mock model that streams deterministic output.
pub fn mock_model() -> Streamed<MockModel> {
    Streamed::new(MockModel::new("mock-model"))
}

/// Returns a credential resolver with a dummy OpenRouter key.
pub fn mock_credentials() -> Arc<CredentialResolver> {
    let mut creds = CredentialResolver::new();
    creds.set_override("OPENROUTER_API_KEY", "dummy-key-for-mock");
    Arc::new(creds)
}

/// Returns a model catalog with a single OpenRouter mock model.
pub fn model_catalog() -> ModelCatalog {
    let providers = vec![ProviderSource::new(
        "openrouter",
        ProviderInfo {
            api_url: "https://openrouter.ai/api/v1".into(),
            env_vars: vec!["OPENROUTER_API_KEY".into()],
            api_type: ProviderType::OpenRouter,
        },
    )];
    let info = ModelInfo {
        modalities: Modality::TEXT,
        max_input: 128_000,
        max_output: 16_384,
        temperature: Some(1.0),
        top_p: Some(0.95),
    };
    let models: Vec<ProviderModelSource<'_>> = [("cutie/patootie", info)]
        .into_iter()
        .map(|(key, i)| ProviderModelSource::new(ProviderIdx::new(0), key, i))
        .collect();
    ModelCatalog::build(&providers, &models).expect("catalog fixture should build")
}

/// Resolves the repository workspace root.
pub fn workspace_root() -> Arc<Path> {
    Arc::from(resolve_workspace_root().expect("resolve workspace root"))
}
