# reloaded-code-models-dev

[![Crates.io](https://img.shields.io/crates/v/reloaded-code-models-dev.svg)](https://crates.io/crates/reloaded-code-models-dev) [![Docs.rs](https://docs.rs/reloaded-code-models-dev/badge.svg)](https://docs.rs/reloaded-code-models-dev)

Sync the online [models.dev] catalog into a compact `ModelCatalog` with
ETag caching, zstd compression, and offline fallback. ~3000 models in ~24 KiB.

[Documentation] · [API Reference]

## Why this exists

If you run coding agents against many providers, you want to have fresh data.
[models.dev][models.dev_link] is one such source of data.

This crate downloads from [models.dev], keeps only the fields we need, and
builds a `reloaded_code_core::models::ModelCatalog`.

## Usage

### Load flow (simple)

1. Read cache header (if present) and get the old ETag.
2. Send request to [models.dev] with `If-None-Match` when ETag exists.
3. If server returns `304 Not Modified`, load catalog from cache.
4. If server returns `200 OK`, parse JSON, map it into catalog sources,
   write fresh cache, then build catalog.
5. If network fails, try cached data as fallback; if no valid cache exists,
   return an error.

### Non-blocking (`tokio`)

```rust
use reloaded_code_models_dev::{CatalogLoadSource, ModelsDevCatalog};

#[cfg(feature = "tokio")]
async fn load_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let result = ModelsDevCatalog::load().await?;

    match result.source {
        CatalogLoadSource::Downloaded => {
            println!("Downloaded fresh catalog data.")
        }
        CatalogLoadSource::NotModifiedCache => {
            println!("Cache is already up to date.")
        }
        CatalogLoadSource::FallbackCache => {
            println!("Network unavailable, using cached catalog data.")
        }
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
use reloaded_code_models_dev::{CatalogLoadSource, ModelsDevCatalog};

#[cfg(feature = "blocking")]
fn load_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let result = ModelsDevCatalog::load()?;

    match result.source {
        CatalogLoadSource::Downloaded => {
            println!("Downloaded fresh catalog data.")
        }
        CatalogLoadSource::NotModifiedCache => {
            println!("Cache is already up to date.")
        }
        CatalogLoadSource::FallbackCache => {
            println!("Network unavailable, using cached catalog data.")
        }
    }

    if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
        println!("provider api url: {}", entry.0.api_url);
        println!("max input tokens: {}", entry.1.max_input);
    }

    Ok(())
}
```

### Load from a custom cache path

```rust
use reloaded_code_models_dev::ModelsDevCatalog;
use std::path::PathBuf;

#[cfg(feature = "tokio")]
async fn load_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let cache_path = PathBuf::from("/tmp/models-dev.cache");
    let _result = ModelsDevCatalog::load_at(&cache_path).await?;
    Ok(())
}

#[cfg(feature = "blocking")]
fn load_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let cache_path = PathBuf::from("/tmp/models-dev.cache");
    let _result = ModelsDevCatalog::load_at(&cache_path)?;
    Ok(())
}
```

### Resolve the shared cache path

```rust
use reloaded_code_models_dev::shared_cache_path;

fn print_cache_path() -> Result<(), Box<dyn std::error::Error>> {
    let path = shared_cache_path()?;
    println!("{}", path.display());
    Ok(())
}
```

## Cache location

By default, cache is stored in the platform cache directory:

- Linux: `~/.cache/reloaded-code/models.dev.catalog.v1.cache`
- macOS: `~/Library/Caches/reloaded-code/models.dev.catalog.v1.cache`
- Windows: `%LOCALAPPDATA%\reloaded-code\models.dev.catalog.v1.cache`

Set `RELOADED_CODE_MODELS_DEV_CACHE_PATH` to override this path.

## Cache size and performance

Current ballpark from a recent [models.dev/api.json] snapshot:

- Size: about `1.31 MiB` JSON -> `109 KiB` serialized payload -> `23.7 KiB` compressed cache
- Compression: about `10.1 ms` with current [zstd] level `17`
- Decompression: about `0.057 ms` (`57 us`) in `--release`
- Cache load into `ModelCatalog`: about `0.31 ms` (`read + decompress + decode + build`)

Measured on a single core of a Ryzen `9950X3D`; these are rough guidance numbers and will drift as the upstream catalog changes.

## Feature flags

- `tokio` (default): async runtime support.
- `blocking`: synchronous runtime support.

Exactly one runtime mode must be enabled.

## License

Apache-2.0

[models.dev]: https://models.dev
[models.dev/api.json]: https://models.dev/api.json
[models.dev_link]: https://models.dev
[zstd]: https://facebook.github.io/zstd/
[Documentation]: https://reloaded-project.github.io/ReloadedCode/models-catalog
[API Reference]: https://docs.rs/reloaded-code-models-dev
