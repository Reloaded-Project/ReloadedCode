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
use reloaded_code_core::{CredentialResolver, TaskInput, resolve_workspace_root};
use reloaded_code_models_dev::ModelsDevCatalog;
use reloaded_code_serdesai::{AgentBuildContext, AgentDefaults};
use serdes_ai::{AgentStreamEvent, UserContent};
use std::{
    fmt::Write,
    io::{self, Write as IoWrite},
    path::PathBuf,
    sync::Arc,
};

const AGENT_NAME: &str = "orchestrator";
const MODEL_ID: &str = "synthetic/hf:zai-org/GLM-4.7-Flash";
const API_KEY_NAME: &str = "SYNTHETIC_API_KEY";
const API_KEY_VALUE: &str = ""; // <-- Set your API key here

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

    println!("\n=== Transcript (message ids, streamed where possible) ===");
    log_xml(0, "user", &prompt_text);

    let mut stream = agent.run_stream(prompt, ()).await?;
    let mut current_message_id = 0u32;
    let mut request_count = 0u32;
    let mut tool_call_count = 0u32;
    // Tracks the currently-open streaming XML tag so we can append deltas without reopening.
    let mut open_tag: Option<OpenStreamTag> = None;
    let mut pending_tool_calls = Vec::with_capacity(4);

    while let Some(event) = stream.next().await {
        match event? {
            AgentStreamEvent::RequestStart { step } => {
                close_stream_xml(&mut open_tag);
                current_message_id = step;
                request_count = request_count.saturating_add(1);
            }
            AgentStreamEvent::ThinkingDelta { text } => {
                write_stream_delta(&mut open_tag, current_message_id, "thinking", &text);
            }
            AgentStreamEvent::TextDelta { text } => {
                write_stream_delta(&mut open_tag, current_message_id, "assistant", &text);
            }
            AgentStreamEvent::ToolCallStart {
                tool_name,
                tool_call_id,
            } => {
                close_stream_xml(&mut open_tag);
                log_xml(current_message_id, "tool", &tool_name);
                pending_tool_calls.push(PendingToolCall {
                    message_id: current_message_id,
                    tool_name,
                    tool_call_id,
                    args: String::new(),
                });
            }
            AgentStreamEvent::ToolCallDelta {
                delta,
                tool_call_id,
            } => {
                // Accumulate streamed JSON args into the matching pending call.
                if let Some(call) =
                    find_pending_tool_call_mut(&mut pending_tool_calls, tool_call_id.as_deref())
                {
                    call.args.push_str(&delta);
                }
            }
            AgentStreamEvent::ToolCallComplete { tool_call_id, .. } => {
                tool_call_count = tool_call_count.saturating_add(1);
                if let Some(call) =
                    take_pending_tool_call(&mut pending_tool_calls, tool_call_id.as_deref())
                {
                    let tag = if call.tool_name == "task" {
                        "task-input"
                    } else {
                        "tool-input"
                    };
                    let content = render_tool_input(&call.tool_name, &call.args);
                    log_xml(call.message_id, tag, &content);
                }
            }
            AgentStreamEvent::ResponseComplete { .. } => {
                close_stream_xml(&mut open_tag);
            }
            AgentStreamEvent::RunComplete { .. } => {
                close_stream_xml(&mut open_tag);
            }
            _ => {}
        }
    }

    close_stream_xml(&mut open_tag);

    println!(
        "Root agent activity: {} model requests, {} tool calls",
        request_count, tool_call_count
    );

    Ok(())
}

fn render_user_content(content: &UserContent) -> String {
    match content {
        UserContent::Text(text) => text.clone(),
        UserContent::Parts(_) => serde_json::to_string_pretty(content)
            .expect("user content serialization should succeed"),
    }
}

fn log_xml(message_id: u32, tag: &str, content: &str) {
    // Long or multiline content gets block-style tags; short content fits on one line.
    if content.contains('\n') || content.len() > 120 {
        println!("<m{message_id}:{tag}>");
        println!("{content}");
        println!("</{tag}>");
        return;
    }

    let mut line = String::with_capacity(content.len() + tag.len() * 2 + 18);
    let _ = write!(line, "<m{message_id}:{tag}>{content}</{tag}>");
    println!("{line}");
}

fn close_stream_xml(open_tag: &mut Option<OpenStreamTag>) {
    if let Some(tag) = open_tag.take() {
        println!();
        println!("</{}>", tag.tag);
    }
}

fn write_stream_delta(
    open_tag: &mut Option<OpenStreamTag>,
    message_id: u32,
    tag: &'static str,
    text: &str,
) {
    if text.is_empty() {
        return;
    }

    // If the message or tag changed, close the previous open tag and start a new one.
    let is_same = open_tag
        .as_ref()
        .is_some_and(|t| t.message_id == message_id && t.tag == tag);
    if !is_same {
        close_stream_xml(open_tag);
        println!("<m{message_id}:{tag}>");
        *open_tag = Some(OpenStreamTag { message_id, tag });
    }

    print!("{text}");
    let _ = io::stdout().flush();
}

struct OpenStreamTag {
    message_id: u32,
    tag: &'static str,
}

struct PendingToolCall {
    message_id: u32,
    tool_name: String,
    tool_call_id: Option<String>,
    args: String,
}

fn find_pending_tool_call_mut<'a>(
    pending: &'a mut [PendingToolCall],
    tool_call_id: Option<&str>,
) -> Option<&'a mut PendingToolCall> {
    // Most providers include a tool_call_id; fall back to the last pending call otherwise.
    match tool_call_id {
        Some(tool_call_id) => pending
            .iter_mut()
            .rev()
            .find(|call| call.tool_call_id.as_deref() == Some(tool_call_id)),
        None => pending.last_mut(),
    }
}

fn take_pending_tool_call(
    pending: &mut Vec<PendingToolCall>,
    tool_call_id: Option<&str>,
) -> Option<PendingToolCall> {
    let index = match tool_call_id {
        Some(tool_call_id) => pending
            .iter()
            .rposition(|call| call.tool_call_id.as_deref() == Some(tool_call_id)),
        None => pending.len().checked_sub(1),
    }?;
    Some(pending.remove(index))
}

fn render_tool_input(tool_name: &str, args_text: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(args_text) {
        Ok(args) if tool_name == "task" => render_task_input(&args),
        Ok(args) => {
            serde_json::to_string_pretty(&args).expect("tool args serialization should succeed")
        }
        Err(_) => args_text.to_string(),
    }
}

fn render_task_input(args: &serde_json::Value) -> String {
    // Try to decode into the typed TaskInput shape; fall back to raw JSON.
    serde_json::from_value::<TaskInput>(args.clone())
        .and_then(|input| serde_json::to_string_pretty(&input))
        .unwrap_or_else(|_| {
            serde_json::to_string_pretty(args).expect("task args serialization should succeed")
        })
}
