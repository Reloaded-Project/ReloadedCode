//! SystemPromptBuilder example - building a complete rig agent.
//!
//! Demonstrates:
//! - Using SystemPromptBuilder with rig's agent builder
//! - Chained .tool() calls for registering tools
//! - TodoTools with shared state
//! - Generating and using the system prompt string
//!
//! Run: cargo run --example rig-basic -p llm-coding-tools-rig

use llm_coding_tools_rig::absolute::{GlobTool, GrepTool, ReadTool};
use llm_coding_tools_rig::{BashTool, SystemPromptBuilder, TodoTools};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::openai::CompletionsClient;

// Set your OpenAI API key here or via OPENAI_API_KEY environment variable.
const OPENAI_API_KEY: &str = "";
const OPENAI_MODEL: &str = "hf:zai-org/GLM-4.7";
const OPENAI_BASE_URL: &str = "https://api.synthetic.new/openai/v1";

fn get_openai_api_key() -> String {
    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| OPENAI_API_KEY.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // === Create shared state for todos ===
    let todos = TodoTools::new();

    // === Create system prompt builder to track tools ===
    let mut pb = SystemPromptBuilder::new()
        .working_directory(std::env::current_dir()?.display().to_string());

    // === Build agent with chained .tool() calls ===
    let client: CompletionsClient = CompletionsClient::builder()
        .api_key(&get_openai_api_key())
        .base_url(OPENAI_BASE_URL)
        .build()?;
    let agent = client
        .agent(OPENAI_MODEL)
        .tool(pb.track(ReadTool::<true>::new()))
        .tool(pb.track(GlobTool::new()))
        .tool(pb.track(GrepTool::<true>::new()))
        .tool(pb.track(BashTool::new()))
        // Todo tools share state for read/write coordination
        .tool(pb.track(todos.read))
        .tool(pb.track(todos.write))
        .preamble(&pb.build())
        .build();

    // === Use the agent ===
    let response = agent
        .prompt("What files are in the current directory?")
        .await?;
    println!("{response}");

    Ok(())
}
