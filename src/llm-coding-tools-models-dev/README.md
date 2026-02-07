# llm-coding-tools-models-dev

Bundled models.dev catalog snapshot and lookup API.

This crate provides a standalone catalog for models.dev provider and model data, with embedded snapshot support and filtering capabilities.

## Features

- Bundled zstd-compressed snapshot (level 22)
- Load from bundled, cached, or downloaded sources
- Model → provider index with optional filtering
- Provider metadata lookup
- Deterministic JSON output for vendored snapshots

## Usage

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use llm_coding_tools_models_dev::{ModelsDevCatalog, CatalogSource};
use std::collections::HashSet;

// Load bundled snapshot
let (catalog, source) = ModelsDevCatalog::from_bundled()?;
assert!(matches!(source, CatalogSource::Bundled));

// Resolve providers for a model
let providers = catalog.resolve_provider_for_model("gpt-4o");
if let Some(provider_ids) = providers {
    for provider_id in provider_ids {
        if let Some(metadata) = catalog.get_provider(provider_id) {
            println!("Provider: {} - env: {:?}", metadata.id, metadata.env);
        }
    }
}

// Load with model filtering
let mut filter = HashSet::new();
filter.insert("gpt-4o".to_string());
let (catalog, _) = ModelsDevCatalog::from_bundled_filtered(&filter)?;
# Ok(())
# }
```

## Update Binary

Regenerate the vendored snapshot from models.dev:

```bash
cargo run -p llm-coding-tools-models-dev --bin models-dev-update
```

This fetches the latest data from <https://models.dev/api.json> and writes a minimal snapshot to `data/models.dev.min.json`.

## License

Apache-2.0
