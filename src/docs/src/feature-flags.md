# Feature Flags

reloaded-code uses Cargo feature flags to control runtime mode, platform
support, and provider availability. This page documents every feature flag
across all crates.

## reloaded-code-core

| Flag               | Default    | Description                                                     |
| ------------------ | ---------- | --------------------------------------------------------------- |
| `tokio`            | yes        | Async runtime support. Enables async tool functions with tokio. |
| `blocking`         | no         | Sync/blocking mode. Tool functions compile as synchronous.      |
| `async`            | (internal) | Base async signatures. Enabled by `tokio`, not set directly.    |
| `linux-bubblewrap` | no         | Linux bubblewrap sandboxing for shell commands. Linux only.     |

`tokio` and `blocking` are mutually exclusive.

## reloaded-code-serdesai

| Flag                | Default | Description                             |
| ------------------- | ------- | --------------------------------------- |
| `full`              | yes     | Enables all 15 provider features below. |
| `openai`            | no      | OpenAI Completions API                  |
| `anthropic`         | no      | Anthropic Claude API                    |
| `azure`             | no      | Azure OpenAI API                        |
| `bedrock`           | no      | AWS Bedrock                             |
| `chatgpt-oauth`     | no      | ChatGPT OAuth                           |
| `claude-code-oauth` | no      | Claude Code OAuth                       |
| `cohere`            | no      | Cohere API                              |
| `gemini`            | no      | Google Gemini API                       |
| `google`            | no      | Google AI API                           |
| `groq`              | no      | Groq API                                |
| `huggingface`       | no      | HuggingFace API                         |
| `mistral`           | no      | Mistral API                             |
| `ollama`            | no      | Ollama (local models)                   |
| `openrouter`        | no      | OpenRouter API                          |
| `antigravity`       | no      | Antigravity API                         |
| `linux-bubblewrap`  | no      | Linux sandbox support                   |

When `full` is enabled, all providers are available. Disable `default-features`
and enable only the providers you need to reduce compile time and binary size:

```toml
[dependencies]
reloaded-code-serdesai = { version = "0.2", default-features = false, features = ["openai", "anthropic"] }
```

## reloaded-code-agents

No feature flags. The crate is feature-free.

## reloaded-code-bubblewrap

| Flag       | Default | Description                           |
| ---------- | ------- | ------------------------------------- |
| `tokio`    | yes     | Async wrapped command execution       |
| `blocking` | no      | Synchronous wrapped command execution |

Compile-time guard: produces a `compile_error!` on non-Linux targets.

## reloaded-code-models-dev

| Flag       | Default | Description                        |
| ---------- | ------- | ---------------------------------- |
| `tokio`    | yes     | Async catalog loading with reqwest |
| `blocking` | no      | Synchronous catalog loading        |

Exactly one must be enabled.

## Common patterns

### Minimal async agent ([SerdesAI], OpenAI only)

```toml
[dependencies]
reloaded-code-serdesai = { version = "0.2", default-features = false, features = ["openai"] }
```

### Full async agent with Linux sandboxing

```toml
[dependencies]
reloaded-code-serdesai = { version = "0.2", features = ["linux-bubblewrap"] }
```

### Framework-agnostic, blocking mode

```toml
[dependencies]
reloaded-code-core = { version = "0.2", default-features = false, features = ["blocking"] }
```

### All providers, no sandboxing

```toml
[dependencies]
reloaded-code-serdesai = "0.2"  # 'full' is default
```

[SerdesAI]: https://crates.io/crates/serdes-ai
