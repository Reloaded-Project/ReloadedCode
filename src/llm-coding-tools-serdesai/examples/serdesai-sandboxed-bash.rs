//! Example with Sandboxed 'bash' tool using `bwrap` on Linux.
//!
//! Demonstrates explicit sandboxed shell execution with [`BashTool`] and a
//! `public_bot` bubblewrap profile, including one non-shell binary lookup.
//!
//! This example creates a `TempDir`-owned sandbox root with `home`, `cache`,
//! and `host-tmp` subdirectories. It bind-mounts `host-tmp` into sandbox
//! `/tmp`, and the whole tree is cleaned up when the `TempDir` drops at
//! process exit.
//!
//! Run:
//! `SYNTHETIC_API_KEY=... cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai`

#[cfg(not(all(feature = "linux-bubblewrap", target_os = "linux")))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("This example requires Linux and the `linux-bubblewrap` feature.");
    eprintln!(
        "Run: SYNTHETIC_API_KEY=... cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai"
    );
    Ok(())
}

#[cfg(all(feature = "linux-bubblewrap", target_os = "linux"))]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use futures::StreamExt;
    use llm_coding_tools_serdesai::agent_ext::AgentBuilderExt;
    use llm_coding_tools_serdesai::{
        BashTool, SystemPromptBuilder,
        profile::{Availability, Builder, TmpBacking},
    };
    use serdes_ai::prelude::*;
    use serdes_ai_models::OpenAIChatModel;
    use std::fmt::Write;
    use std::fs;

    const API_KEY_NAME: &str = "SYNTHETIC_API_KEY";
    const API_KEY_VALUE: &str = "";
    const MODEL_ID: &str = "hf:zai-org/GLM-4.7-Flash";
    const BASE_URL: &str = "https://api.synthetic.new/openai/v1";

    fn get_api_key() -> String {
        std::env::var(API_KEY_NAME).unwrap_or_else(|_| API_KEY_VALUE.to_string())
    }

    fn log_xml(request_id: u32, tag: &str, content: &str) {
        let mut line = String::with_capacity(content.len() + tag.len() * 2 + 18);
        let _ = write!(line, "<{request_id}:{tag}>{content}</{tag}>");
        println!("{line}");
    }

    let api_key = get_api_key();
    if api_key.is_empty() {
        eprintln!("Set {API_KEY_NAME} or edit API_KEY_VALUE before running this example.");
        return Ok(());
    }

    let availability = Availability::detect();
    if let Some(reason) = availability.reason() {
        eprintln!("bubblewrap is unavailable: {reason}");
        return Ok(());
    }

    let workspace = std::env::current_dir()?;
    let sandbox_root = tempfile::Builder::new()
        .prefix("llm-coding-tools-serdesai-sandboxed-bash-")
        .tempdir()?;
    let synthetic_home = sandbox_root.path().join("home");
    let cache_root = sandbox_root.path().join("cache");
    let host_tmp = sandbox_root.path().join("host-tmp");
    fs::create_dir_all(&synthetic_home)?;
    fs::create_dir_all(&cache_root)?;
    fs::create_dir_all(&host_tmp)?;

    let profile = Builder::public_bot(
        &*workspace,
        &*synthetic_home,
        &*cache_root,
        Some(TmpBacking::BindHost(host_tmp.clone().into_boxed_path())),
    )
    .with_availability(availability)
    .build()?;

    let bash = BashTool::host()
        .with_linux_bwrap(profile)
        .with_default_timeout(std::time::Duration::from_secs(20))
        .with_default_workdir(&workspace);

    let mut pb = SystemPromptBuilder::new().working_directory(workspace.display().to_string());
    let model = OpenAIChatModel::new(MODEL_ID, api_key).with_base_url(BASE_URL);
    let agent = AgentBuilder::<(), String>::new(model)
        .instructions(
            "Use bash exactly once, rely on its output, and explain briefly why the result shows sandboxed execution.",
        )
        .tool(pb.track(bash))
        .system_prompt(pb.build())
        .build();

    println!(
        "=== Sandboxed Bash Agent Ready ({} tools) ===",
        agent.tools().len()
    );
    println!("Profile: public_bot");
    println!("Workspace: {}", workspace.display());
    println!("Sandbox root: {}", sandbox_root.path().display());
    println!("Synthetic home: {}", synthetic_home.display());
    println!("Cache root: {}", cache_root.display());
    println!("Host tmp bound to /tmp: {}", host_tmp.display());

    println!("\n=== Running Agent ===");
    let prompt = "Use bash exactly once to run `printf 'PWD=%s\\nHOME=%s\\nCAT=%s\\n' \"$PWD\" \"$HOME\" \"$(command -v cat)\" && printf 'hello-through-cat\\n' | cat` and then explain briefly why the result shows a sandboxed shell with an extra system binary available.";
    let mut stream = agent.run_stream(prompt, ()).await?;

    let mut request_id = 0u32;
    log_xml(request_id, "user", prompt);
    request_id = request_id.saturating_add(1);
    let mut assistant_message = String::with_capacity(256);

    while let Some(event) = stream.next().await {
        match event? {
            AgentStreamEvent::TextDelta { text, .. } => assistant_message.push_str(&text),
            AgentStreamEvent::RequestStart { .. } => assistant_message.clear(),
            AgentStreamEvent::ToolCallStart { tool_name, .. } => {
                log_xml(request_id, "tool", &tool_name);
                request_id = request_id.saturating_add(1);
            }
            AgentStreamEvent::ResponseComplete { .. } => {
                log_xml(request_id, "assistant", &assistant_message);
                request_id = request_id.saturating_add(1);
            }
            _ => {}
        }
    }

    Ok(())
}
