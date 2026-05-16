//! Demonstrates loading custom provider configuration from files and programmatic entries.
//!
//! ```sh
//! cargo run -p reloaded-code-provider-config --example config-loader
//! ```

use reloaded_code_provider_config::{ModelConfig, ProviderConfig, ProviderConfigLoader};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Pre-load conventional config paths (user-global then project-local).
    // Only files that exist on disk are included.
    let mut loader = ProviderConfigLoader::with_default_paths()?;

    // Add a programmatic provider entry (loaded last, overrides any file entry with the same key).
    loader.add_provider(
        "example-provider",
        ProviderConfig {
            api_url: Some("https://api.example.com/v1".into()),
            api_type: Some("openai-compatible".into()),
            env: Some(vec!["EXAMPLE_API_KEY".into()]),
            models: Some({
                let mut m = indexmap::IndexMap::new();
                m.insert(
                    "example-model".to_string(),
                    ModelConfig {
                        max_input: 128000,
                        max_output: 8192,
                        modalities: vec!["text".to_string()],
                        default_temperature: None,
                        default_top_p: None,
                    },
                );
                m
            }),
        },
    );

    let loaded = loader.load()?;
    let (providers, models) = loaded.to_catalog_sources()?;

    println!(
        "Loaded {} providers and {} models",
        providers.len(),
        models.len()
    );
    for ps in &providers {
        println!(
            "  provider: {} ({:?})",
            ps.provider_key, ps.provider.api_type
        );
    }
    for ms in &models {
        println!(
            "  model: {} (max_input={}, max_output={})",
            ms.model_key, ms.model.max_input, ms.model.max_output
        );
    }

    Ok(())
}
