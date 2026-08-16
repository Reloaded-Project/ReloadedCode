# reloaded-code-provider-config

YAML-based custom provider configuration for ReloadedCode.

Parse provider definitions from YAML files, merge multiple sources, and
convert them into catalog types for `ModelCatalog::build()`.

## Install

```toml
[dependencies]
reloaded-code-provider-config = "0.1.0"
```

## Usage

```rust
use reloaded_code_provider_config::ProviderConfigLoader;

let loaded = ProviderConfigLoader::with_default_paths()?.load()?;
for (key, config) in &loaded.providers {
    let count = config.models.as_ref().map_or(0, |m| m.len());
    println!("{key}: {count} model(s)");
}
```

See the [project documentation] for details.

[project documentation]: https://github.com/Reloaded-Project/ReloadedCode
