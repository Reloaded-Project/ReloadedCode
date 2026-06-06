# Custom Providers

Define new LLM providers via YAML configuration files - no Rust code required.

## Conventional config paths

!!! note "Opt-in defaults"
    These paths are conventions, not hard-coded lookup locations.
    The library never reads config files unless the application
    explicitly asks it to.

    - [`ProviderConfigLoader::with_default_paths()`][with_default_paths]
      resolves and loads both paths in order (user-global, then
      project-local).
    - [`ProviderConfigLoader::add_path()`][add_path] loads a single
      custom path.

    Use [`default_config_paths()`][default_config_paths] if you only
    need the path list without loading.

When using the defaults, later files override earlier ones per provider key:

| Path                                     | Scope         |
| ---------------------------------------- | ------------- |
| `~/.config/reloaded-code/providers.yaml` | User-global   |
| `.reloaded/providers.yaml`               | Project-local |

When using the defaults, you can have zero, one, or both files present.
If neither exists, only the models.dev catalog is used (no custom providers).

[`default_config_paths()`][default_config_paths] is also available to resolve the conventional
paths without loading them.

## YAML format

```yaml
my-llm:
  api_url: https://api.myllm.com/v1
  api_type: openai-compatible
  env:
    - MY_LLM_API_KEY
  models:
    MiniMax-M2.7:
      max_input: 204800
      max_output: 131072
      modalities: [text]
```

Each provider must include at least one model under `models`.

### Provider fields

| Field        | Type        | Default             | Notes                                 |
| ------------ | ----------- | ------------------- | ------------------------------------- |
| `api_url`    | string      | required            | Base URL for the API endpoint         |
| `api_type`   | string      | `openai-compatible` | Maps to provider behaviour profile    |
| `env`        | string list | `[]`                | Env var names checked for credentials |
| `models`     | map         | required            | Models offered by this provider       |

### api_type values

| Value               | Provider type                                |
| ------------------- | -------------------------------------------- |
| `openai`            | OpenAI (chat completions)                    |
| `openai-compatible` | Any OpenAI-API-compatible endpoint (default) |
| `openai-responses`  | OpenAI Responses API                         |
| `anthropic`         | Anthropic                                    |
| `google`            | Google/Gemini                                |
| `groq`              | Groq                                         |
| `mistral`           | Mistral                                      |
| `ollama`            | Ollama                                       |
| `bedrock`           | AWS Bedrock                                  |
| `azure`             | Azure                                        |
| `openrouter`        | OpenRouter                                   |
| `huggingface`       | Hugging Face                                 |
| `cohere`            | Cohere                                       |

Omit `api_type` to default to `openai-compatible`.

`openai` and `openai-compatible` both map to OpenAI chat completions.
`openai` signals actual OpenAI; `openai-compatible` signals any other
OpenAI-API-compatible endpoint.

### Model fields

| Field                 | Type        | Default  | Notes                                           |
| --------------------- | ----------- | -------- | ----------------------------------------------- |
| `max_input`           | u32         | required | Context window / input limit                    |
| `max_output`          | u32         | required | Output token limit                              |
| `modalities`          | string list | `[text]` | Supported modalities: text, image, audio, video |
| `default_temperature` | f32         | -        | Default sampling temperature                    |
| `default_top_p`       | f32         | -        | Default nucleus sampling                        |

## Credentials

Custom providers use the existing `CredentialResolver` - no separate
resolution path needed.

The `env` field lists environment variable names to check, in order.
At runtime, `CredentialResolver` checks its overrides first, then falls
back to those env vars.

```yaml
my-llm:
  api_url: https://api.myllm.com/v1
  env:
    - MY_LLM_API_KEY
    - MY_LLM_TOKEN  # fallback
```

For providers with no `env` entry (e.g., local endpoints like Ollama
behind a compatibility layer), no API key is required.

## Rust API

```rust
use reloaded_code_provider_config::{ModelConfig, ProviderConfig, ProviderConfigLoader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Option A: Use conventional paths (user-global then project-local).
    let mut loader = ProviderConfigLoader::with_default_paths()?;

    // Option B: Full control - choose paths yourself.
    // let mut loader = ProviderConfigLoader::new();
    // loader.add_path(".reloaded/providers.yaml")?;

    // Add a programmatic entry (loaded last, overrides any file entry with the same key).
    loader.add_provider("my-llm", ProviderConfig {
        api_url: Some("https://api.myllm.com/v1".into()),
        api_type: Some("openai-compatible".into()),
        env: Some(vec!["MY_LLM_API_KEY".into()]),
        models: Some({
            let mut m = indexmap::IndexMap::new();
            m.insert("my-model".to_string(), ModelConfig {
                max_input: 128000,
                max_output: 8192,
                modalities: vec!["text".to_string()],
                default_temperature: None,
                default_top_p: None,
            });
            m
        }),
    });

    let loaded = loader.load()?;
    let (providers, models) = loaded.to_catalog_sources();

    // Pass to ModelCatalog::build() alongside models.dev sources.
    // let catalog = ModelCatalog::build(&providers, &models)?;
    Ok(())
}
```

## Merge Behaviour

When multiple config sources define the same provider key, the **later**
source completely replaces the earlier entry. There is no deep merge of
model maps - the entire provider entry is replaced.

```yaml
# File 1: ~/.config/reloaded-code/providers.yaml
my-llm:
  api_url: https://api.myllm.com/v1
  models:
    v1: { max_input: 128000, max_output: 8192 }

# File 2: .reloaded/providers.yaml (loaded later, wins)
my-llm:
  api_url: https://api.myllm.com/v2
  models:
    v2: { max_input: 256000, max_output: 16384 }
```

Result: only `v2` model exists under `my-llm` - the `v1` model from
file 1 is fully replaced.

[with_default_paths]: https://docs.rs/reloaded-code-provider-config/latest/reloaded_code_provider_config/struct.ProviderConfigLoader.html#method.with_default_paths
[add_path]: https://docs.rs/reloaded-code-provider-config/latest/reloaded_code_provider_config/struct.ProviderConfigLoader.html#method.add_path
[default_config_paths]: https://docs.rs/reloaded-code-provider-config/latest/reloaded_code_provider_config/fn.default_config_paths.html
