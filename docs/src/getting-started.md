# Getting Started

Build sandboxed coding agents in Rust. Define agents in markdown, attach
tools with permissions, and run them against any LLM provider. You'll need
a Rust project and an LLM API key (e.g. `OPENAI_API_KEY`).

## Build your first agent

=== "With Agent Files"

    !!! info "Agents are defined as markdown files with YAML frontmatter"
        The agent file format mirrors [OpenCode]'s agent definition format -
        similar enough that many files are drop-in compatible, but
        [not identical](migration.md).

    **1.** Create an agent file at `agents/coder.md`:

    ```markdown
    ---
    name: coder
    mode: all
    description: A coding agent that can read, search, and edit files.
    permission:
      read: allow
      write: allow
      edit: allow
      glob: allow
      grep: allow
      bash: allow
      webfetch: allow
      task: deny
    ---

    You are a coding assistant. Use the available tools to complete the user's task.
    ```

    **2.** Add the dependencies:
    ```toml
    [dependencies]
    llm-coding-tools-serdesai = "0.2"
    llm-coding-tools-agents = "0.1"
    llm-coding-tools-core = "0.2"
    llm-coding-tools-models-dev = "0.1"
    tokio = { version = "1", features = ["full"] }
    ```

    **3.** Run the agent:

    ```rust
    use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
    use llm_coding_tools_core::CredentialResolver;
    use llm_coding_tools_models_dev::ModelsDevCatalog;
    use llm_coding_tools_serdesai::{AgentBuildContext, AgentDefaults};
    use std::{path::PathBuf, sync::Arc};

    #[tokio::main]
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        // Load agent definitions from markdown files
        let mut catalog = AgentCatalog::new();
        AgentLoader::new().add_directory(&mut catalog, "./agents")?;

        // Sync the models.dev catalog (with ETag caching and offline fallback)
        let load_result = ModelsDevCatalog::load().await?;

        // Build runtime with a default model and the loaded agents
        let runtime = AgentRuntimeBuilder::new()
            .catalog(catalog)
            .defaults(AgentDefaults::with_model("synthetic/hf:MiniMaxAI/MiniMax-M2.5"))
            .build()?;

        // Create a shared build context (catalog + credentials)
        // API keys and endpoints are resolved automatically by matching the
        // model string against the models.dev catalog (e.g. OPENAI_API_KEY).
        let build_context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(load_result.catalog),
            Arc::new(CredentialResolver::new()),
        );

        // Build a named agent and run it
        let agent = build_context.build("coder")?;
        let response = agent.run("Find all TODO comments in src/", ()).await?;
        println!("{}", response.output());
        Ok(())
    }
    ```

=== "Without Agent Files"

    For simpler use cases, attach tools directly to a [SerdesAI] agent
    builder (the LLM agent framework):

    ```toml
    [dependencies]
    llm-coding-tools-serdesai = "0.2"
    llm-coding-tools-core = "0.2"
    tokio = { version = "1", features = ["full"] }
    ```

    ```rust
    use llm_coding_tools_core::CredentialResolver;
    use llm_coding_tools_serdesai::{
        ReadTool, GlobTool, GrepTool, EditTool, AbsolutePathResolver,
        BashTool, SystemPromptBuilder, WebFetchTool, create_todo_tools,
        agent_ext::AgentBuilderExt,
    };
    use serdes_ai::prelude::*;
    use serdes_ai_models::OpenAIChatModel;

    #[tokio::main]
    async fn main() -> Result<(), Box<dyn std::error::Error>> {
        let (todo_read, todo_write, _) = create_todo_tools();
        let mut pb = SystemPromptBuilder::new()
            .working_directory("/path/to/project".to_string());

        let credentials = CredentialResolver::new();
        let api_key = credentials.resolve("OPENAI_API_KEY")
            .expect("OPENAI_API_KEY not set");

        let model = OpenAIChatModel::new("hf:zai-org/GLM-4.7-Flash", api_key)
            .with_base_url("https://api.synthetic.new/openai/v1");

        let agent = AgentBuilder::<(), String>::new(model)
            .tool(pb.track(ReadTool::new(AbsolutePathResolver)))
            .tool(pb.track(GlobTool::new(AbsolutePathResolver)))
            .tool(pb.track(GrepTool::new(AbsolutePathResolver)))
            .tool(pb.track(EditTool::new(AbsolutePathResolver)))
            .tool(pb.track(BashTool::host()))
            .system_prompt(pb.build())
            .build();

        let response = agent.run("Find all TODO comments", ()).await?;
        println!("{}", response.output());
        Ok(())
    }
    ```

!!! note "What just happened?"

    - **Agent markdown** (with agent files) defines the agent's name, permissions
      (default-deny), and system prompt in one file
    - **SystemPromptBuilder** (without agent files) generates the system prompt
      with guidance for every attached tool
    - **CredentialResolver** resolves API keys from environment variables or
      explicit overrides (see below)
    - **AgentBuildContext** (with agent files) wires the model catalog,
      credentials, and agent definitions together
    - **`build("coder")`** resolves the agent by name, attaches its permitted
      tools, and generates the system prompt

!!! tip "Runnable examples"
    The repository includes complete examples for both paths:
    [serdesai-basic](https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-basic.rs)
    (without agent files) and
    [serdesai-agents](https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-serdesai/examples/serdesai-agents.rs)
    (with agent files). See [Examples](guides/examples.md) for the full list.

## Credential management

`CredentialResolver` resolves API keys by name (e.g. `"OPENAI_API_KEY"`) -
overrides first, then environment variables. The resolver skips empty values,
so an empty override falls through to the environment variable.

```rust
use llm_coding_tools_core::CredentialResolver;

let mut resolver = CredentialResolver::new();
resolver.set_override("OPENAI_API_KEY", "sk-...");
```

For multi-tenant servers or shared CI runners where environment variables
should be ignored, use `CredentialResolver::without_env()`.

## Run the examples

The repository ships with complete, runnable examples:

```bash
# Basic agent setup
cargo run --example serdesai-basic -p llm-coding-tools-serdesai

# Sandboxed file access (restricted to allowed directories)
cargo run --example serdesai-sandboxed -p llm-coding-tools-serdesai

# Sandboxed bash execution (Linux, requires bubblewrap)
cargo run --example serdesai-sandboxed-bash --features linux-bubblewrap -p llm-coding-tools-serdesai

# Agent catalog loading from markdown files
cargo run --example serdesai-agents -p llm-coding-tools-serdesai

# Multi-agent task delegation (orchestrator delegates to sub-agents)
cargo run --example serdesai-task -p llm-coding-tools-serdesai
```

See [Examples](guides/examples.md) for the full list with descriptions and
source links.

## Sandboxing for production

For production deployments handling untrusted input, enable sandboxing:

```toml
[dependencies]
llm-coding-tools-serdesai = { version = "0.2", features = ["linux-bubblewrap"] }
```

Use `AllowedPathResolver` to restrict file access and the [bubblewrap] sandbox
to isolate shell execution. See [Sandboxing](sandboxing.md) for the full guide.

### Common deployment profiles

- **Discord bot / chat bot** - Use the Public Bot sandbox profile
  (restrictive; see [Sandboxing](sandboxing.md#the-two-profiles)) and
  `AllowedPathResolver` to limit what the LLM can do with user-provided prompts.
- **CI/CD pipeline** - Use the Trusted Maintenance profile
  (permissive; see [Sandboxing](sandboxing.md#the-two-profiles)) for build jobs
  where you control the inputs. Explicitly mount the cache directories so that
  build artifacts persist between runs.

If you use a framework other than SerdesAI, see [Custom Framework
Integration](guides/custom-framework.md).

## Blocking mode

All crates default to async via the `tokio` feature.

To use blocking mode, disable default features and enable `blocking`:

```toml
[dependencies]
llm-coding-tools-core = { version = "0.2", default-features = false, features = ["blocking"] }
```

## Next steps

- [Tools](tools.md) - every tool's behaviour, inputs, and outputs
- [Agents](agents.md) - define agents with markdown files and YAML frontmatter
- [Architecture](architecture.md) - understand how the 5 crates fit together

[SerdesAI]: https://crates.io/crates/serdes-ai
[OpenCode]: https://opencode.ai/
[bubblewrap]: https://github.com/containers/bubblewrap
