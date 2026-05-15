# Custom Providers (YAML-based provider configuration)

## Summary

Add `reloaded-code-provider-config` crate for defining custom LLM providers via YAML files - no Rust code required. Also fixes OpenAI-compatible providers to work without credentials when no env vars are listed (e.g., local Ollama).

## Changes

### New crate: `reloaded-code-provider-config`

- **loader.rs** - `ProviderConfigLoader` collects YAML files and programmatic entries, merges them (later source wins), validates, and produces catalog sources
- **config.rs** - Serde shapes for `ProviderConfig` and `ModelConfig`
- **api_type.rs** - Maps `api_type` strings to `ProviderType` variants
- **error.rs** - Typed errors for validation and I/O failures

Conventional config paths (opt-in via `with_default_paths()`):
- `~/.config/reloaded-code/providers.yaml` (user-global)
- `.reloaded/providers.yaml` (project-local)

### Core changes

- **`Modality::from_label()`** - Parses `"text"`, `"image"`, `"audio"`, `"video"` into `Modality` bitflags
- **Provider bridge fix** - OpenAI-compatible providers without credential env vars now work with empty API key

### Documentation

- New guide: `docs/src/guides/custom-providers.md`
- Updated nav, index, models-catalog, and examples pages

## YAML schema

```yaml
my-llm:
  api_url: https://api.myllm.com/v1
  api_type: openai-compatible   # optional, defaults to "openai-compatible"
  env:                          # optional, credential env var names
    - MY_LLM_API_KEY
  models:
    my-model:
      max_input: 128000         # required
      max_output: 8192          # required
      modalities: [text, image] # optional, defaults to [text]
      default_temperature: 0.7  # optional
      default_top_p: 0.95       # optional
```

Supported `api_type`: `openai`, `openai-compatible`, `openai-responses`, `anthropic`, `google`, `groq`, `mistral`, `ollama`, `bedrock`, `azure`, `openrouter`, `huggingface`, `cohere`.

## Test coverage

- Config deserialization (full, minimal, multi-provider)
- Loader (single file, empty, override semantics, programmatic entries)
- Validation (missing fields, unrecognized api_type/modality, malformed YAML)
- Catalog conversion (ProviderType mapping, ProviderIdx consistency)
- Provider bridge (keyless endpoints succeed, credential-required still enforced)


