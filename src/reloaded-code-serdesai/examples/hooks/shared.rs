// Fixture module included into each example binary via `#[path]`; binaries
// use different subsets of these helpers, so unused ones are expected.
#![allow(dead_code)]

use reloaded_code_agents::{
    AgentCatalog, AgentConfig, AgentDefaults, AgentMode, AgentRuntimeBuilder, PermissionRule,
};
use reloaded_code_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
    ProviderSource, ProviderType,
};
use reloaded_code_core::permissions::PermissionAction;
use reloaded_code_core::{CredentialResolver, HookSet, resolve_workspace_root};
use reloaded_code_serdesai::AgentBuildContext;
use reloaded_code_serdesai::mock::{FunctionModel, Streamed};
use serdes_ai::core::{FinishReason, ModelResponse, ModelResponsePart, ToolReturnPart};
use serdes_ai_models::MockModel;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

const DEFAULT_MODEL_ID: &str = "openrouter/cutie/patootie";
/// Contents of the `service.env` fixture. The redaction demo scrubs the
/// `API_KEY=` and `TOKEN=` lines, so the benign `LOG_LEVEL` line must stay
/// to show that plain lines survive the scrub.
const SERVICE_ENV: &str = "\
# Demo service configuration.
API_KEY=sk-demo-9f41c2a7b8e04d65
TOKEN=tok-demo-6b83d15a92f0
LOG_LEVEL=debug
";

/// Hermetic temp workspace for tool-hook examples.
///
/// File tools resolve paths inside [`TempWorkspace::root`] only, so example
/// runs cannot read or write the repository checkout. Dropping the fixture
/// deletes the workspace, so keep it alive for the whole agent run.
pub struct TempWorkspace {
    /// Temp directory guard; dropping it deletes the workspace and its files.
    pub dir: TempDir,
    /// Workspace root, ready to pass to [`AgentBuildContext::new`] or
    /// [`build_agent_context_in_workspace`].
    pub root: Arc<Path>,
    /// Config fixture whose `API_KEY=`/`TOKEN=` lines the redaction demo scrubs.
    pub secrets_file: PathBuf,
    /// Write target that no example reads first; the fixture never creates it.
    pub unread_target: PathBuf,
}

/// Builds an `AgentConfig` fixture whose permission rules allow the named tools.
///
/// Agents attach only the standard tools their permission rules explicitly
/// allow, so tool-using examples need this variant instead of
/// [`agent_config`].
///
/// # Arguments
///
/// - `name` - the agent name.
/// - `description` - the agent description.
/// - `prompt` - the agent system prompt.
/// - `tools` - names of the standard tools the agent may call.
pub fn agent_config_with_tools(
    name: &str,
    description: &str,
    prompt: &str,
    tools: &[&str],
) -> AgentConfig {
    let permission = tools
        .iter()
        .map(|tool| {
            (
                (*tool).into(),
                PermissionRule::Action(PermissionAction::Allow),
            )
        })
        .collect();
    AgentConfig {
        permission,
        ..agent_config(name, description, prompt)
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

/// Builds an [`AgentBuildContext`] wired to mock models and credentials,
/// exposing `workspace_root` to tools instead of the repository root.
///
/// Tool examples pass [`TempWorkspace::root`] so `read` and `write` calls
/// stay inside the temp workspace.
///
/// # Arguments
///
/// - `catalog` - the agent catalog to attach to the runtime.
/// - `hooks` - the hook set to install on the runtime.
/// - `workspace_root` - project directory exposed to tools.
///
/// # Panics
///
/// Panics when the agent runtime fails to build.
pub fn build_agent_context_in_workspace(
    catalog: AgentCatalog,
    hooks: HookSet,
    workspace_root: Arc<Path>,
) -> AgentBuildContext {
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
        workspace_root,
    )
}

/// Returns a mock model that streams deterministic output.
pub fn mock_model() -> Streamed<MockModel> {
    Streamed::new(MockModel::new("mock-model"))
}

/// Creates a hermetic temp workspace containing the example fixture files.
///
/// Tool grants still come from [`agent_config_with_tools`]; this fixture only
/// owns the directory and its files.
///
/// # Panics
///
/// Panics when the temp directory or a fixture file cannot be created.
pub fn temp_workspace() -> TempWorkspace {
    let dir = TempDir::new().expect("create temp workspace");
    let root: Arc<Path> = Arc::from(dir.path());
    let secrets_file = dir.path().join("service.env");
    std::fs::write(&secrets_file, SERVICE_ENV).expect("write service.env fixture");
    let unread_target = dir.path().join("draft.md");
    TempWorkspace {
        dir,
        root,
        secrets_file,
        unread_target,
    }
}

/// Builds a mock model that scripts two tool calls, then answers with text.
///
/// The first turn calls `first`; once its tool return is fed back, the next
/// turn calls `second`; once both tool returns are in the conversation, the
/// model answers with `final_text` followed by the real tool returns. This
/// gives two-step examples the same deterministic shape that
/// `mock::tool_then_text` gives single-call examples.
///
/// # Arguments
///
/// - `first` - name and JSON arguments of the tool called on the first turn.
/// - `second` - name and JSON arguments of the tool called on the second turn.
/// - `final_text` - text prefix for the closing response; the real tool
///   returns are appended after it.
///
/// # Example
///
/// ```rust,no_run
/// use serde_json::json;
///
/// let model = two_tools_then_text(
///     ("read", json!({"file_path": "service.env"})),
///     ("write", json!({"file_path": "draft.md", "content": "notes"})),
///     "Run finished.",
/// );
/// ```
pub fn two_tools_then_text(
    first: (impl Into<String>, serde_json::Value),
    second: (impl Into<String>, serde_json::Value),
    final_text: impl Into<String>,
) -> Streamed<FunctionModel> {
    let first_tool: String = first.0.into();
    let first_args = first.1;
    let second_tool: String = second.0.into();
    let second_args = second.1;
    let final_text = final_text.into();

    let model = FunctionModel::new(move |messages, _settings| {
        // Each finished tool call adds one tool return to the history, so
        // their count tells which scripted turn is next.
        let answered_calls = messages.iter().flat_map(|m| m.tool_returns()).count();

        match answered_calls {
            0 => tool_call_response(&first_tool, &first_args),
            1 => tool_call_response(&second_tool, &second_args),
            _ => {
                let tool_results: String = messages
                    .iter()
                    .flat_map(|m| m.tool_returns())
                    .map(tool_return_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if tool_results.is_empty() {
                    final_text.clone()
                } else {
                    format!("{final_text}\n\n{tool_results}")
                };
                ModelResponse::text(text)
            }
        }
    });

    Streamed::new(model)
}

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

/// Emits a tool-call response with the shape `mock::tool_then_text` uses:
/// a short text part, then the call that triggers the real tool.
fn tool_call_response(tool_name: &str, args: &serde_json::Value) -> ModelResponse {
    ModelResponse::with_parts(vec![
        ModelResponsePart::text(format!("Calling {tool_name}...")),
        ModelResponsePart::tool_call(tool_name, args.clone()),
    ])
    .with_finish_reason(FinishReason::ToolCall)
}

/// Extracts human-readable text from a tool return part.
///
/// Tool returns feed content back as tagged JSON, so round-trip through
/// `serde_json` and keep the readable payload; tool implementations and
/// hook-supplied responses both use plain text content.
fn tool_return_text(part: &ToolReturnPart) -> String {
    let Ok(value) = serde_json::to_value(&part.content) else {
        return format!("{:?}", part.content);
    };
    if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| format!("{:?}", part.content))
}
