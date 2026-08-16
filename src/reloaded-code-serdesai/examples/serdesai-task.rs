//! Stateless Task delegation example using the models.dev catalog.
//!
//! Loads markdown agents from `examples/agents/task-demo/`, builds the primary
//! orchestrator through [`AgentBuildContext::build`], and runs one
//! prompt that should delegate exactly once to `reader`.
//!
//! Run: Edit the API_KEY_NAME and API_KEY_VALUE constants below, then:
//!      cargo run --example serdesai-task -p reloaded-code-serdesai

use futures::StreamExt;
use reloaded_code_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use reloaded_code_core::{CredentialResolver, resolve_workspace_root};
use reloaded_code_models_dev::ModelsDevCatalog;
use reloaded_code_serdesai::{AgentBuildContext, AgentDefaults, RunEvent};
use serdes_ai::UserContent;
use std::{
    fmt::Write,
    io::{self, Write as IoWrite},
    path::PathBuf,
    sync::Arc,
};

const AGENT_NAME: &str = "orchestrator";
const API_KEY_NAME: &str = "SYNTHETIC_API_KEY";
const API_KEY_VALUE: &str = ""; // <-- Set your API key here
const MODEL_ID: &str = "synthetic/hf:zai-org/GLM-4.7-Flash";

struct OpenStreamTag {
    tag: &'static str,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agents_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("agents")
        .join("task-demo");
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
    let loader = AgentLoader::new();
    loader.add_file(&mut catalog, agents_dir.join("orchestrator.md"))?;
    loader.add_file(&mut catalog, agents_dir.join("reader.md"))?;

    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog)
        .defaults(AgentDefaults::with_model(MODEL_ID))
        .build()?;
    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(credentials),
        Arc::from(resolve_workspace_root()?),
    );

    println!(
        "Loading named agent `{AGENT_NAME}` from {}",
        agents_dir.display()
    );
    let agent = build_context.build(AGENT_NAME)?;
    println!(
        "Built `{AGENT_NAME}` on demand with {} tools.",
        agent.tools().len()
    );

    let prompt = format!(
        "If the model supports visible reasoning output, think briefly before acting, then ask `reader` to give a short summary of {}.",
        readme_path.display(),
    );
    let prompt = UserContent::text(prompt);
    let prompt_text = render_user_content(&prompt);

    println!("\n=== Transcript (streamed where possible) ===");
    log_xml("user", &prompt_text);

    let mut stream = agent.run_stream(prompt, ()).await?;
    let mut tool_call_count = 0u32;
    // Tracks the currently-open streaming XML tag so we can append deltas without reopening.
    let mut open_tag: Option<OpenStreamTag> = None;

    while let Some(event) = stream.next().await {
        match event? {
            RunEvent::ThinkingDelta { text } => {
                write_stream_delta(&mut open_tag, "thinking", &text);
            }
            RunEvent::TextDelta { text } => {
                write_stream_delta(&mut open_tag, "assistant", &text);
            }
            RunEvent::ToolCallStart { tool_name, .. } => {
                close_stream_xml(&mut open_tag);
                log_xml("tool", &tool_name);
            }
            RunEvent::ToolCallComplete { .. } => {
                tool_call_count = tool_call_count.saturating_add(1);
            }
            RunEvent::RunComplete { .. } => {
                close_stream_xml(&mut open_tag);
            }
            _ => {}
        }
    }

    close_stream_xml(&mut open_tag);

    println!("Root agent activity: {tool_call_count} tool calls");

    Ok(())
}

fn log_xml(tag: &str, content: &str) {
    // Long or multiline content gets block-style tags; short content fits on one line.
    if content.contains('\n') || content.len() > 120 {
        println!("<{tag}>");
        println!("{content}");
        println!("</{tag}>");
        return;
    }

    let mut line = String::with_capacity(content.len() + tag.len() * 2 + 18);
    let _ = write!(line, "<{tag}>{content}</{tag}>");
    println!("{line}");
}

fn render_user_content(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Parts(_) => serde_json::to_string_pretty(content)
            .expect("user content serialization should succeed"),
    }
}

fn write_stream_delta(open_tag: &mut Option<OpenStreamTag>, tag: &'static str, text: &str) {
    if text.is_empty() {
        return;
    }

    // If the tag changed, close the previous open tag and start a new one.
    let is_same = open_tag.as_ref().is_some_and(|t| t.tag == tag);
    if !is_same {
        close_stream_xml(open_tag);
        println!("<{tag}>");
        *open_tag = Some(OpenStreamTag { tag });
    }

    print!("{text}");
    let _ = io::stdout().flush();
}

fn close_stream_xml(open_tag: &mut Option<OpenStreamTag>) {
    if let Some(tag) = open_tag.take() {
        println!();
        println!("</{}>", tag.tag);
    }
}
