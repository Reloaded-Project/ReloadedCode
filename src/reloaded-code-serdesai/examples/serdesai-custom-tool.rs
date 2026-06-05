//! Portable custom tool example using the models.dev catalog.
//!
//! Loads a markdown agent, registers a framework-neutral custom tool through
//! [`AgentRuntimeBuilder`], builds the SerdesAI agent with [`AgentBuildContext`],
//! and runs a prompt that should call the custom tool.
//!
//! Run: Edit the API_KEY_NAME and API_KEY_VALUE constants below, then:
//!      cargo run --example serdesai-custom-tool -p reloaded-code-serdesai

use reloaded_code_agents::{AgentCatalog, AgentDefaults, AgentLoader, AgentRuntimeBuilder};
use reloaded_code_core::context::{ToolContext, ToolPrompt};
use reloaded_code_core::{
    CredentialResolver, CustomTool, CustomToolDefinition, CustomToolFuture, ToolBuildContext,
    ToolCatalogEntry, ToolCatalogKind, ToolFactory, ToolOutput, ToolResult, ToolRunContext,
    default_tools, resolve_workspace_root,
};
use reloaded_code_models_dev::ModelsDevCatalog;
use reloaded_code_serdesai::AgentBuildContext;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const AGENT_NAME: &str = "custom-tool-demo";
const MODEL_ID: &str = "synthetic/hf:zai-org/GLM-4.7-Flash";
const API_KEY_NAME: &str = "SYNTHETIC_API_KEY";
const API_KEY_VALUE: &str = ""; // <-- Set your API key here
const PROJECT_INFO_TOOL: &str = "project_info";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agents_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("agents")
        .join("custom-tool");

    let mut credentials = CredentialResolver::without_env();
    if !API_KEY_VALUE.is_empty() {
        credentials.set_override(API_KEY_NAME, API_KEY_VALUE);
    }

    // Load model catalog from models.dev (online-first with local cache fallback).
    let load_result = ModelsDevCatalog::load().await?;
    println!(
        "Loaded model catalog from models.dev (source: {:?})",
        load_result.source
    );

    let mut catalog = AgentCatalog::new();
    AgentLoader::new().add_file(&mut catalog, agents_dir.join("custom-tool-demo.md"))?;

    let workspace_root = resolve_workspace_root()?;
    let mut tools = default_tools();
    tools.push(ToolCatalogEntry::new(
        PROJECT_INFO_TOOL,
        ToolCatalogKind::Custom,
    ));

    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog)
        .defaults(AgentDefaults::with_model(MODEL_ID))
        .tools(tools)
        .custom_tool(ProjectInfoFactory)
        .build()?;

    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(credentials),
        Arc::from(workspace_root.as_path()),
    );

    println!("Building `{AGENT_NAME}` with portable custom tool `{PROJECT_INFO_TOOL}`.");
    let agent = build_context.build(AGENT_NAME)?;
    println!("Built `{AGENT_NAME}` with {} tools.", agent.tools().len());

    let prompt = "Call project_info with include_examples=true, then summarize what it says in three bullets.";
    let response = agent.run(prompt, ()).await?;
    println!("{}", response.output());

    Ok(())
}

/// Factory registered with the framework-agnostic runtime.
struct ProjectInfoFactory;

impl ToolContext for ProjectInfoFactory {
    fn name(&self) -> &'static str {
        PROJECT_INFO_TOOL
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(
            "Use project_info to inspect demo metadata exposed by the host application.",
        )
    }
}

impl ToolFactory for ProjectInfoFactory {
    fn create(&self, ctx: &ToolBuildContext) -> ToolResult<Arc<dyn CustomTool>> {
        Ok(Arc::new(ProjectInfoTool {
            workspace_root: ctx.workspace_root().to_path_buf(),
            manifest_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        }))
    }
}

/// The portable custom tool implementation.
///
/// This type depends only on `reloaded-code-core`, not SerdesAI. Other framework
/// adapters can wrap the same `CustomTool` object in their native tool trait.
struct ProjectInfoTool {
    workspace_root: PathBuf,
    manifest_dir: PathBuf,
}

impl ToolContext for ProjectInfoTool {
    fn name(&self) -> &'static str {
        PROJECT_INFO_TOOL
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Static(
            "Use project_info to inspect demo metadata exposed by the host application.",
        )
    }
}

impl CustomTool for ProjectInfoTool {
    fn definition(&self) -> CustomToolDefinition {
        CustomToolDefinition::new(
            PROJECT_INFO_TOOL,
            "Return host-provided metadata about this repository and custom tool demo.",
        )
        .with_parameters(json!({
            "type": "object",
            "properties": {
                "include_examples": {
                    "type": "boolean",
                    "description": "Include the names of SerdesAI example files."
                }
            },
            "additionalProperties": false
        }))
    }

    fn call<'a>(
        &'a self,
        ctx: ToolRunContext<'a>,
        args: serde_json::Value,
    ) -> CustomToolFuture<'a> {
        Box::pin(async move {
            let include_examples = args
                .get("include_examples")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

            let mut lines = vec![
                format!("workspace_root: {}", self.workspace_root.display()),
                format!("serdesai_manifest_dir: {}", self.manifest_dir.display()),
                format!(
                    "called_by_model: {}",
                    ctx.model_name().unwrap_or("<unknown model>")
                ),
                format!("run_id_present: {}", ctx.run_id().is_some()),
                format!("tool_call_id_present: {}", ctx.tool_call_id().is_some()),
            ];

            if include_examples {
                let examples = list_example_files(&self.manifest_dir)?;
                lines.push(format!("serdesai_examples: {}", examples.join(", ")));
            }

            Ok(ToolOutput::new(lines.join("\n")))
        })
    }
}

fn list_example_files(manifest_dir: &Path) -> ToolResult<Vec<String>> {
    let examples_dir = manifest_dir.join("examples");
    let mut names = Vec::new();

    for entry in std::fs::read_dir(examples_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
            && let Some(name) = path.file_name().and_then(|name| name.to_str())
        {
            names.push(name.to_owned());
        }
    }

    names.sort_unstable();
    Ok(names)
}
