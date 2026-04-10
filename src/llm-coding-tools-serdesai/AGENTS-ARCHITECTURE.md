# Architecture: llm-coding-tools-serdesai (Agent Runtime)

[SerdesAI] adapter that builds runnable [`serdes_ai::Agent`] instances from the
framework-agnostic [`AgentRuntime`] provided by `llm-coding-tools-agents`.

The crate also contains standalone tools (read, write, edit, glob, grep,
bash, webfetch, todo) and Linux [bubblewrap] sandboxing. This document focuses
on the **agent runtime** subsystem.

For the foundation crate, see
[llm-coding-tools-agents/ARCHITECTURE.md](https://github.com/Sewer56/llm-coding-tools/blob/main/src/llm-coding-tools-agents/ARCHITECTURE.md).

## Table of Contents

- [Quick Start](#quick-start)
- [Phase 1: Building Agents](#phase-1-building-agents)
  - [Building the Context (Setup)](#building-the-context-setup)
  - [Building a SerdesAI Agent (Runtime)](#building-a-serdesai-agent-runtime)
  - [Shared: prepare_build()](#shared-prepare_build)
    - [Model Resolution: `resolve_model_with_catalog`](#model-resolution-resolve_model_with_catalog)
    - [Provider Bridge: `build_serdes_model`](#provider-bridge-build_serdes_model)
- [Task Delegation](#task-delegation)
  - [Depth Guard](#depth-guard)
- [Reference](#reference)
  - [Error Model](#error-model)
  - [File Map](#file-map)

## Quick Start

Build and run an agent from markdown definitions:

```rust
use llm_coding_tools_agents::{AgentCatalog, AgentLoader, AgentRuntimeBuilder};
use llm_coding_tools_core::CredentialResolver;
use llm_coding_tools_models_dev::ModelsDevCatalog;
use llm_coding_tools_serdesai::{AgentBuildContext, AgentDefaults};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load model catalog (models.dev crate)
    let load_result = ModelsDevCatalog::load().await?;
    
    // 2. Load agent definitions (agents crate)
    let mut catalog = AgentCatalog::new();
    AgentLoader::new().add_directory(&mut catalog, "~/.opencode")?;
    
    // 3. Build runtime with defaults (agents crate + serdesai defaults)
    let runtime = AgentRuntimeBuilder::new()
        .catalog(catalog)
        .defaults(AgentDefaults::with_model("synthetic/hf:zai-org/GLM-4.7-Flash"))
        .build();
    let build_context = AgentBuildContext::new(
        Arc::new(runtime),
        Arc::new(load_result.catalog),
        Arc::new(CredentialResolver::without_env()),
    );
    
    // 4. Build and run the agent (serdesai crate - converts runtime -> runnable agent)
    let agent = build_context.build("code-reviewer")?;
    let response = agent.run("Review this code", ()).await?; // run via serdes
    println!("{}", response.output());
    
    Ok(())
}
```

## Phase 1: Building Agents

Transform framework-agnostic agent configurations into runnable [SerdesAI]
agents with tools.

`AgentBuildContext::build()` is the single public build entrypoint.
The process splits into two phases:

### Building the Context (Setup)

Create a reusable build context once with shared resources:

```text
   Arc<AgentRuntime> + Arc<ModelCatalog> + Arc<Credentials>
                         │
                         ▼
                AgentBuildContext::new(...)
                         │
                         ▼
            AgentBuildContext
            ├─ Arc<AgentRuntime>      (agent catalog + config)
            ├─ Arc<ModelCatalog>      (model definitions)
            └─ Arc<CredentialResolver> (API credentials)
```

This context holds references to shared resources and can build multiple agents.

### Building a SerdesAI Agent (Runtime)

Build individual [SerdesAI] agents from the context (can be called multiple times).
Transforms the framework-agnostic [`AgentConfig`] into a runnable [SerdesAI]
[`Agent`]:

```text
   AgentBuildContext::build("agent-name")
                         │
                         ▼
                    build_agent()
                         │
                         ▼
                    prepare_build()
            ┌─────────────────────────┐
            │ 1. Load AgentConfig     │
            │ 2. resolve_model()      │
            │ 3. build_serdes_model() │
            │ 4. allowed_tools()      │
            │ 5. Summarize targets    │
            └─────────────────────────┘
                         │
                         ▼
               Depth guard check
             (clears targets if at limit)
                         │
                         ▼
           attach_standard_tools()
            (TaskTool only if targets exist)
                         │
                         ▼
               SerdesAI Agent<(), String>
```

Results in a runnable [SerdesAI] `Agent<(), String>` ready to call `.run()`.

Internally shares the build context (via Arc) so delegated sub-agents can
recursively build each other at runtime.

### Shared: prepare_build()

Central helper that gathers all configuration from the runtime catalog
([`AgentConfig`]) to construct a runnable [SerdesAI] [`Agent`].

```text
prepare_build(runtime, name, model_catalog, credentials)
    │
    ▼
1. Load config    -> AgentConfig (by name)
2. Resolve model  -> ResolvedModel
3. Build model    -> BoxedModel
4. Get tools      -> Vec<ToolCatalogEntry>
5. Summarize      -> Vec<TaskTargetSummary>
    │
    ▼
PreparedBuild {
    agent_name, model, prompt,
    temperature, top_p,
    tools,
    callable_target_summaries,
}
```

#### Model Resolution: `resolve_model_with_catalog`

Resolves which model an agent should use by checking agent override, then
runtime defaults, validating against the model catalog:

```text
   resolve_model_with_catalog(model_catalog, defaults, agent)
                │
                │  1. Check agent.model override
                │     └─ not set? check defaults.model
                │  2. Parse "provider/model-id"
                │  3. Validate provider exists in catalog
                │  4. Validate model exists in catalog
                ▼
           ┌────────────────┐
           │ ResolvedModel  │  provider + model
           └────────────────┘
```

Precedence: **agent override** wins over **runtime default**.

#### Provider Bridge: `build_serdes_model`

Connects framework-agnostic [`ResolvedModel`] to concrete [SerdesAI]
[`BoxedModel`] implementations:

```text
   ResolvedModel { provider, model }
        │
        │  catalog.lookup_provider(provider)
        ▼
   ProviderInfo { api_url, env_vars, api_type }
        │
        │  match api_type
        ▼
   build_serdes_model()
        │
        │  credential resolution:
        │    require_env_value()          finds first _API_KEY / _TOKEN
        │    first_matching_env_value()   optional values (region, project_id)
        ▼
   ResolvedSerdesModel { model: BoxedModel, spec: "provider:model" }
```

Each provider function is gated behind a feature flag. When a feature is
disabled, model construction returns a clear configuration error telling the
user which flag to enable.

| ProviderType      | SerdesAI Model       | Notes               |
| ----------------- | -------------------- | ------------------- |
| OpenAiCompletions | OpenAIChatModel      |                     |
| OpenAiResponses   | OpenAIResponsesModel |                     |
| Anthropic         | AnthropicModel       |                     |
| Google            | GoogleModel          |                     |
| Groq              | GroqModel            | fixed endpoint      |
| Mistral           | MistralModel         |                     |
| Ollama            | OllamaModel          | no credential       |
| Bedrock           | BedrockModel         | AWS credentials     |
| Azure             | AzureOpenAIModel     | endpoint + key      |
| OpenRouter        | OpenRouterModel      | fixed endpoint      |
| HuggingFace       | HuggingFaceModel     |                     |
| Cohere            | CohereModel          | fixed endpoint      |
| ChatGptOAuth      | ChatGptOAuthModel    | access token        |
| ClaudeCodeOAuth   | ClaudeCodeOAuthModel | access token        |
| Antigravity       | AntigravityModel     | token + project_id  |
| Unknown           | ModelError           | configuration error |

## Task Delegation

How agents delegate work to sub-agents at runtime via the `task` tool.
With depth limits and validation.

```text
   LLM emits tool_call("task", { subagent_type, prompt, description })
        │
        ▼
   TaskTool::call()
        │
        ▼
    TaskHandle::execute(caller_name, input)
        │
        ├─ check task_settings.allows_delegation(current_depth)
        │    └─ exceeded? -> ValidationFailed("max_task_depth reached")
        ├─ validate_target(caller_name, target_name)
        │    ├─ caller in catalog?       -> no  -> ExecutionFailed
        │    ├─ target in catalog?       -> no  -> ValidationFailed("unknown")
        │    ├─ target.mode == Primary?  -> yes -> ValidationFailed("primary")
        │    └─ permission.task configured + disallows?
        │         └─ yes -> ValidationFailed("not allowed")
        │
        ├─ build_agent(context, target_name, depth+1)
        │    └─ recursive: builds sub-agent with its own tools
        │       (TaskTool included only if sub-agent has callable targets
        │        and depth < max_task_depth)
        │
        └─ agent.run(prompt, ())
             └─ response text -> TaskOutput
```

### Depth Guard

`TaskSettings::max_depth` (default: 3) limits delegation hops:

```text
   depth 0  ->  orchestrator (has TaskTool)
   depth 1  ->  sub-agent-a  (has TaskTool if max_depth > 1)
   depth 2  ->  sub-agent-b  (has TaskTool if max_depth > 2)
   depth 3  ->  leaf-agent   (no TaskTool at default max_depth=3)
```

Two defenses:

1. **Build time**: `build_agent()` clears `callable_target_summaries` when disabled,
   so `attach_standard_tools()` skips TaskTool.
2. **Runtime**: `TaskHandle::execute()` re-checks depth (defense-in-depth).

## Reference

### Error Model

Build-time: `AgentBuildError` - catalog issues (UnknownAgent), model resolution
failures (ModelResolutionError), or model init errors (ModelInit).
Runtime: `ToolError::ValidationFailed` (unknown/primary target, permission denied,
max depth) or `ToolError::ExecutionFailed` (sub-agent build/run failure).

### File Map

```text
llm-coding-tools-serdesai/src/
├── agent_runtime/
│   ├── mod.rs              module root, re-exports from llm-coding-tools-agents
│   ├── build.rs            prepare_build(), attach_standard_tools()
│   │                         AgentBuildError
│   ├── model.rs            resolve_model() - thin wrapper delegating to agents crate
│   ├── task.rs             AgentBuildContext (public)
│   │                         build_agent() internals (private)
│   └── provider_bridge/
│       ├── mod.rs          build_serdes_model() - ProviderType -> concrete SerdesAI model
│       └── tests.rs        provider bridge integration tests
├── task/
│   ├── mod.rs              module root, re-exports
│   ├── definition.rs       task_tool_definition(), render_task_targets()
│   ├── handle.rs           TaskHandle - validates + executes delegated Task calls
│   └── tool.rs             TaskTool - SerdesAI Tool impl backed by TaskHandle
├── agent_ext.rs            AgentBuilderExt - bridges serdes_ai::tools::Tool -> ToolExecutor
└── lib.rs                  crate root, re-exports
```

[SerdesAI]: https://crates.io/crates/serdes-ai
[bubblewrap]: https://github.com/containers/bubblewrap
