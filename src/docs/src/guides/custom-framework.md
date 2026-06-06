# Custom Framework Integration

Integrate reloaded-code into any Rust LLM framework. Wrap tool
functions, generate system prompts, and configure path security in three
steps. You only need this if you're using a framework other than
[SerdesAI] (an LLM agent framework).

If you're using SerdesAI, see [Getting Started](../getting-started.md)
instead.

## The integration pattern

Every framework adapter does three things:

1. **Wrap core tool functions** into the framework's tool trait
2. **Generate the system prompt** using `SystemPromptBuilder`
3. **Resolve paths** using a `PathResolver` implementation

## Step 1: Wrap a tool function

Here's how you'd wrap the `read_file` function for a hypothetical framework:

```rust
use reloaded_code_core::{
    read_file, PathResolver, ToolResult,
    tools::read::{ReadInput, ReadOutput},
};

// Your framework's tool trait
trait MyTool {
    type Input;
    type Output;
    fn name(&self) -> &str;
    fn execute(&self, input: Self::Input) -> impl std::future::Future<Output = Result<Self::Output, String>>;
}

// Read tool adapter
struct MyReadTool<R: PathResolver> {
    resolver: R,
    line_numbers: bool,
}

impl<R: PathResolver + Clone + Send + Sync + 'static> MyTool for MyReadTool<R> {
    type Input = ReadInput;
    type Output = ReadOutput;

    fn name(&self) -> &str {
        "read"
    }

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, String> {
        let path = self.resolver.resolve(&input.path)
            .map_err(|e| e.to_string())?;

        read_file::<_, true>(&path, input.offset, input.limit)
            .await
            .map_err(|e| e.to_string())
    }
}
```

## Step 2: Generate the system prompt

Use `SystemPromptBuilder` to create a system prompt that includes guidance for
every tracked tool:

```rust
use reloaded_code_core::{
    SystemPromptBuilder, ToolContext,
    context::{PathMode, ToolPrompt},
    tool_metadata,
};

// Implement ToolContext for your tool
impl<R: PathResolver> ToolContext for MyReadTool<R> {
    fn name(&self) -> &'static str {
        tool_metadata::read::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Read {
            path_mode: PathMode::Absolute,
            line_numbers: self.line_numbers,
        }
    }
}

// Build the prompt
let mut pb = SystemPromptBuilder::new()
    .working_directory("/path/to/project".to_string());

let read_tool = MyReadTool {
    resolver: AbsolutePathResolver, // simplest; see Step 3 for sandboxed alternatives
    line_numbers: true,
};
pb.track(read_tool);
// pb.track(other_tool);
// pb.track(another_tool);

// For custom tools (e.g. tool factories, framework adapters) where you
// have name + prompt but no instance, use track_entry():
pb.track_entry("my_custom_tool", ToolPrompt::Static("Use my_custom_tool to do X."));

let system_prompt = pb.build();
```

The builder includes guidance only for tracked tools. Cross-tool references
(e.g. "prefer grep over read for searching") are included only when both tools
are present.

## Portable custom tools

Custom tools implement `CustomTool` in `reloaded-code-core`, not a framework
trait. Your adapter only needs a thin wrapper that:

1. Converts `CustomToolDefinition` into your framework's tool definition type
2. Forwards JSON arguments to `CustomTool::call(ToolRunContext, args)`
3. Converts the returned `ToolOutput` into your framework's tool return type

That means the same custom tool implementation can be registered once through
`ToolFactory` and reused by SerdesAI or any other Rust LLM framework adapter.

!!! tip "Adapter example"

    See SerdesAI's
    [`CustomToolAdapter`](https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-serdesai/src/tools/custom.rs)
    for a concrete adapter implementation, plus
    [`serdesai-custom-tool`](../examples.md#serdesai-integration) for a runnable
    portable custom tool example using the agent runtime, or
    [`serdesai-custom-tool-standalone`](../examples.md#serdesai-integration) for a
    direct `AgentBuilder` example without the runtime.

## Step 3: Choose a path resolver

| Resolver               | Use when                                                                                    |
| ---------------------- | ------------------------------------------------------------------------------------------- |
| `AbsolutePathResolver` | Paths are unrestricted; use when you trust the LLM or restrict access via the system prompt |
| `AllowedPathResolver`  | You want to restrict file access to specific directories                                    |
| `AllowedGlobResolver`  | You want fine-grained glob-based allow/deny rules                                           |

```rust
use reloaded_code_core::{
    AbsolutePathResolver, AllowedPathResolver,
    path::{AllowedGlobResolver, GlobPolicy, RuleAction},
};

// Unrestricted
let any_path = AbsolutePathResolver;

// Directory-restricted
let sandbox = AllowedPathResolver::new(["/workspace/project", "/tmp"])?;

// Glob-filtered (last matching rule takes precedence)
let glob = AllowedGlobResolver::new(["/workspace/project"])?
    .with_policy(
        GlobPolicy::builder()
            .add("src/**", RuleAction::Allow)?
            .add("target/**", RuleAction::Deny)?
            .build()?
    );
```

!!! tip "Runnable example"

    See
    [system_prompt_preview](https://github.com/Reloaded-Project/ReloadedCode/blob/main/src/reloaded-code-core/examples/system_prompt_preview.rs)
    for a working example of prompt building with the core library.
    See [Examples](../examples.md) for the full list.

## What you get from core

| Component                                      | What it provides                                         |
| ---------------------------------------------- | -------------------------------------------------------- |
| `read_file`, `write_file`, `edit_file`         | File operations                                          |
| `glob_files`, `grep_search`                    | Search operations                                        |
| `execute_command`, `execute_command_with_mode` | Shell execution                                          |
| `fetch_url`                                    | URL fetching                                             |
| `read_todos`, `write_todos`                    | Shared todo state                                        |
| `SystemPromptBuilder`                          | Context-aware system prompt generation                   |
| `ToolContext` trait                            | Tool metadata interface for prompt building              |
| `CustomTool`                                   | Portable custom tool definition and execution            |
| `ToolFactory` / `CustomToolRegistry`           | Portable custom tool creation and lookup                 |
| `ToolCatalogEntry` / `ToolCatalogKind`         | Standard/custom tool catalog for adapters                |
| `PathResolver` trait                           | Path security boundary                                   |
| `AllowedPathResolver`                          | Directory-based sandbox                                  |
| `AllowedGlobResolver`                          | Glob-based sandbox (last matching rule takes precedence) |
| `Ruleset` / `Rule`                             | Permission evaluation engine                             |
| `CredentialResolver`                           | API key lookup with overrides                            |
| `ModelCatalog`                                 | Compact provider/model hash table                        |
| `ToolError`                                    | Unified error type for all tools                         |

For the full API reference, see [docs.rs/reloaded-code-core](https://docs.rs/reloaded-code-core).

[SerdesAI]: https://crates.io/crates/serdes-ai
