# llm-coding-tools-models-dev

Reads the online models.dev catalog into llm-coding-tools-core; with support
for a cached fallback and caching via ETag(s).

## Why this exists

If you run coding agents against many providers, you want to have fresh data.
[models.dev][models.dev] is one such source of data.

This crate has the sufficient code to download from models.dev, distill down only
the relevant data we need; and create a llm_coding_tools_core `ModelCatalog`.

## Usage

### Load flow (simple)

1. Read cache header (if present) and get the old ETag.
2. Send request to models.dev with `If-None-Match` when ETag exists.
3. If server returns `304 Not Modified`, load catalog from cache.
4. If server returns `200 OK`, parse JSON, map it into catalog sources, write fresh cache, then build catalog.
5. If network fails, try cached data as fallback; if no valid cache exists, return an error.

### Non-blocking (`tokio`)

```rust
use llm_coding_tools_models_dev::{CatalogLoadSource, ModelsDevCatalog};

#[cfg(feature = "tokio")]
async fn load_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let result = ModelsDevCatalog::load().await?;

    match result.source {
        CatalogLoadSource::Downloaded => println!("Downloaded fresh catalog data."),
        CatalogLoadSource::NotModifiedCache => println!("Cache is already up to date."),
        CatalogLoadSource::FallbackCache => println!("Network unavailable, using cached catalog data."),
    }

    if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
        println!("provider api url: {}", entry.0.api_url);
        println!("max input tokens: {}", entry.1.max_input);
    }

    Ok(())
}
```

### Blocking (`blocking`)

```rust
use llm_coding_tools_models_dev::{CatalogLoadSource, ModelsDevCatalog};

#[cfg(feature = "blocking")]
fn load_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let result = ModelsDevCatalog::load()?;

    match result.source {
        CatalogLoadSource::Downloaded => println!("Downloaded fresh catalog data."),
        CatalogLoadSource::NotModifiedCache => println!("Cache is already up to date."),
        CatalogLoadSource::FallbackCache => println!("Network unavailable, using cached catalog data."),
    }

    if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
        println!("provider api url: {}", entry.0.api_url);
        println!("max input tokens: {}", entry.1.max_input);
    }

    Ok(())
}
```

## Cache location

By default, cache is stored in the platform cache directory:

- Linux: `~/.cache/llm-coding-tools/models.dev.catalog.v1.cache`
- macOS: `~/Library/Caches/llm-coding-tools/models.dev.catalog.v1.cache`
- Windows: `%LOCALAPPDATA%\llm-coding-tools\models.dev.catalog.v1.cache`

Set `LLM_CODING_TOOLS_MODELS_DEV_CACHE_PATH` to override this path.

## Feature flags

- `tokio` (default): async runtime support.
- `blocking`: synchronous runtime support.

Exactly one runtime mode must be enabled.

## License

Apache-2.0

[models.dev]: https://models.dev
