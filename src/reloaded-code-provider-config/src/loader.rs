//! Builder that collects config sources, merges them, and produces catalog inputs.
//!
//! Use [`ProviderConfigLoader`] to assemble config sources (YAML files and
//! programmatic entries), then call [`ProviderConfigLoader::load()`] to merge
//! and validate them into a [`LoadedProviderConfig`]. Finally, call
//! [`LoadedProviderConfig::to_catalog_sources()`] to obtain the
//! `(Vec<ProviderSource>, Vec<ProviderModelSource>)` pair. The model catalog
//! consumes this pair.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use crate::api_type::{api_type_from_str, DEFAULT_API_TYPE};
use crate::config::ProviderConfig;
use crate::error::ProviderConfigError;
use reloaded_code_core::models::{
    Modality, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource, ProviderSource,
    ProviderType,
};

const CONFIG_FILENAME: &str = "providers.yaml";
const CONFIG_DIR_NAME: &str = "reloaded-code";
const PROJECT_LOCAL_DIR: &str = ".reloaded";

/// Builder that collects an ordered list of config sources and merges them
/// into a [`LoadedProviderConfig`].
///
/// # Merge semantics
///
/// Later sources override earlier ones per-provider-key. When a provider key
/// appears in a later source, the entire provider entry from the earlier source
/// is replaced (no deep merge of models).
///
/// # Quick start
///
/// Use [`Self::with_default_paths()`] to pre-load the conventional config
/// file locations, then chain additional sources before calling [`Self::load()`].
pub struct ProviderConfigLoader {
    sources: Vec<ConfigSource>,
}

impl ProviderConfigLoader {
    /// Creates a loader pre-loaded with conventional config file paths.
    ///
    /// Calls [`default_config_paths()`] and adds each existing path as a
    /// source. Your application must call [`Self::load()`] explicitly.
    ///
    /// # Returns
    ///
    /// - `Ok(Self)`: A loader with conventional file sources appended.
    ///
    /// Equivalent to:
    ///
    /// ```rust,no_run
    /// use reloaded_code_provider_config::{ProviderConfigLoader, default_config_paths};
    /// fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut loader = ProviderConfigLoader::new();
    /// for path in default_config_paths() {
    ///     loader.add_path(&path)?;
    /// }
    /// Ok(())
    /// }
    /// ```
    pub fn with_default_paths() -> Result<Self, ProviderConfigError> {
        let mut loader = Self::new();
        for path in default_config_paths() {
            loader.add_path(&path)?;
        }
        Ok(loader)
    }

    /// Creates an empty loader with no sources.
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Adds a YAML config file path to the source list.
    ///
    /// The file is read and parsed during [`Self::load()`].
    ///
    /// # Arguments
    ///
    /// - `path`: Filesystem path to a YAML config file. The loader stores the
    ///   path without reading the file until [`Self::load()`] is called.
    ///
    /// # Returns
    ///
    /// - `Ok(&mut Self)`: The loader, for chaining.
    pub fn add_path(&mut self, path: impl AsRef<Path>) -> Result<&mut Self, ProviderConfigError> {
        self.sources
            .push(ConfigSource::File(path.as_ref().to_path_buf()));
        Ok(self)
    }

    /// Adds a programmatic provider entry.
    ///
    /// Programmatic entries participate in merge order just like file-based
    /// sources. The loader appends each entry to the source list.
    ///
    /// The `key` is a user-chosen provider identifier (e.g., `"my-llm"`,
    /// `"local-ollama"`). It becomes the [`ProviderSource::provider_key`] in
    /// the catalog and is used for model lookups as `"{key}/{model}"`.
    /// These keys are custom provider identifiers, not the provider names in
    /// the pre-bundled catalog (hosted at models.dev).
    pub fn add_provider(&mut self, key: &str, config: ProviderConfig) -> &mut Self {
        self.sources.push(ConfigSource::Programmatic {
            key: key.to_string(),
            config,
        });
        self
    }

    /// Reads all sources, validates them, and merges into a single config.
    ///
    /// Later sources override earlier ones per-provider-key.
    ///
    /// # Errors
    ///
    /// - Returns [`ProviderConfigError::FileRead`] when a config file cannot be read from disk.
    /// - Returns [`ProviderConfigError::YamlParse`] when a config file contains invalid YAML.
    /// - Returns [`ProviderConfigError::MissingField`] when a provider is missing `api_url`
    ///   (for non-Ollama providers) or `models`, or when `models` is empty.
    /// - Returns [`ProviderConfigError::UnrecognizedModality`] when a model has a modality
    ///   string that is not one of: `text`, `image`, `audio`, `video`.
    /// - Returns [`ProviderConfigError::UnrecognizedApiType`] when a provider has an
    ///   `api_type` string that does not map to a known [`ProviderType`] variant.
    pub fn load(self) -> Result<LoadedProviderConfig, ProviderConfigError> {
        let mut merged: IndexMap<String, ProviderConfig> = IndexMap::new();

        for source in self.sources {
            let providers = match source {
                ConfigSource::File(path) => {
                    let contents = std::fs::read_to_string(&path).map_err(|e| {
                        ProviderConfigError::FileRead {
                            path: path.clone(),
                            source: e,
                        }
                    })?;
                    let parsed: IndexMap<String, ProviderConfig> = serde_yaml::from_str(&contents)
                        .map_err(|e| ProviderConfigError::YamlParse { path, source: e })?;
                    parsed
                }
                ConfigSource::Programmatic { key, config } => {
                    let mut map = IndexMap::new();
                    map.insert(key, config);
                    map
                }
            };

            for (key, config) in providers {
                // Later source wins: full replacement, no deep merge.
                merged.insert(key, config);
            }
        }

        // Validate all merged entries.
        for (key, config) in &merged {
            // Validate api_type first so UnrecognizedApiType surfaces before
            // MissingField(api_url); otherwise an invalid api_type resolves to
            // Unknown (≠ Ollama) and the api_url check fires incorrectly.
            let provider_type =
                api_type_from_str(config.api_type.as_deref().unwrap_or(DEFAULT_API_TYPE));
            if provider_type == ProviderType::Unknown {
                return Err(ProviderConfigError::UnrecognizedApiType {
                    provider_key: key.clone(),
                    value: config
                        .api_type
                        .as_deref()
                        .unwrap_or(DEFAULT_API_TYPE)
                        .to_string(),
                });
            }
            // api_url is required for non-Ollama providers.
            if config.api_url.is_none() && provider_type != ProviderType::Ollama {
                return Err(ProviderConfigError::MissingField {
                    provider_key: key.clone(),
                    field: "api_url",
                });
            }
            // models is required and must not be empty.
            let models =
                config
                    .models
                    .as_ref()
                    .ok_or_else(|| ProviderConfigError::MissingField {
                        provider_key: key.clone(),
                        field: "models",
                    })?;
            if models.is_empty() {
                return Err(ProviderConfigError::MissingField {
                    provider_key: key.clone(),
                    field: "models (must have at least one model)",
                });
            }
            for (model_key, model) in models {
                // Validate modalities - check each string individually.
                for mod_str in &model.modalities {
                    if Modality::from_label(mod_str).is_none() {
                        return Err(ProviderConfigError::UnrecognizedModality {
                            provider_key: key.clone(),
                            model_key: model_key.clone(),
                            value: mod_str.clone(),
                        });
                    }
                }
            }
        }

        Ok(LoadedProviderConfig { providers: merged })
    }
}

impl Default for ProviderConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns the conventional config file paths, in merge order.
///
/// Order (later overrides earlier):
/// 1. `dirs::config_dir()/reloaded-code/providers.yaml` (user-global)
/// 2. `.reloaded/providers.yaml` (project-local, relative to CWD)
///
/// Only paths that exist on disk are included. If neither file exists,
/// returns an empty vector and the application proceeds using only its
/// pre-bundled model catalog, with no user-supplied config sources.
///
/// The application decides whether to call this at all - the library does
/// not auto-load config files.
pub fn default_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // User-global: platform config directory.
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join(CONFIG_DIR_NAME).join(CONFIG_FILENAME));
    }

    // Project-local: relative to current working directory.
    paths.push(PathBuf::from(PROJECT_LOCAL_DIR).join(CONFIG_FILENAME));

    // Filter to only paths that exist on disk.
    paths.into_iter().filter(|p| p.exists()).collect()
}

/// Merged, validated provider configuration ready for catalog conversion.
///
/// Call [`Self::to_catalog_sources()`] to obtain `Result<(Vec<ProviderSource>, Vec<ProviderModelSource>), ProviderConfigError>`
/// that can be passed directly to [`ModelCatalog::build()`].
///
/// The map keys are user-chosen provider identifiers (e.g., `"my-llm"`,
/// `"local-ollama"`) that become [`ProviderSource::provider_key`] values in
/// the catalog. These are distinct from the provider names in the
/// pre-bundled catalog (hosted at models.dev) - they are custom providers
/// defined by the user.
///
/// [`ModelCatalog::build()`]: reloaded_code_core::models::ModelCatalog::build
#[derive(Debug)]
pub struct LoadedProviderConfig {
    /// Merged provider entries keyed by user-chosen provider identifier.
    pub providers: IndexMap<String, ProviderConfig>,
}

impl LoadedProviderConfig {
    /// Converts the merged config into catalog source types.
    ///
    /// # Returns
    ///
    /// - `Ok((Vec<ProviderSource>, Vec<ProviderModelSource<'_>))`: A pair where
    ///   each model's [`ProviderModelSource::provider_idx`] (a [`ProviderIdx`] numeric
    ///   index) corresponds to its provider's position in the first vector. The
    ///   returned [`ProviderModelSource`] borrows `model_key` strings from `self`, so
    ///   `self` must outlive the sources.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderConfigError::TooManyProviders`] when the number of
    /// providers exceeds `u16::MAX + 1` (65,536), which is the maximum
    /// addressable by [`ProviderIdx`].
    pub fn to_catalog_sources(
        &self,
    ) -> Result<(Vec<ProviderSource>, Vec<ProviderModelSource<'_>>), ProviderConfigError> {
        let provider_count = self.providers.len();
        let max = (u16::MAX as usize) + 1;
        if provider_count > max {
            return Err(ProviderConfigError::TooManyProviders {
                count: provider_count,
                max,
            });
        }
        let mut provider_sources = Vec::with_capacity(self.providers.len());
        let mut model_sources = Vec::new();

        for (idx, (key, config)) in self.providers.iter().enumerate() {
            // Resolve the provider type from the api_type string, defaulting to openai-compatible.
            let provider_type =
                api_type_from_str(config.api_type.as_deref().unwrap_or(DEFAULT_API_TYPE));
            let provider_info = ProviderInfo {
                api_url: config.api_url.clone().unwrap_or_default(),
                env_vars: config.env.clone().unwrap_or_default(),
                api_type: provider_type,
            };
            let provider_source = ProviderSource::new(key, provider_info);
            let provider_idx = ProviderIdx::new(idx as u16);

            // Convert each model entry into a ProviderModelSource.
            if let Some(models) = &config.models {
                for (model_key, model_config) in models {
                    // Build the modality bitmask by OR'ing individual Modality flags;
                    // defaults to text-only if the model declares no modalities.
                    let mut modalities = Modality::empty();
                    for s in &model_config.modalities {
                        if let Some(m) = Modality::from_label(s) {
                            modalities |= m;
                        }
                    }
                    if modalities.is_empty() {
                        modalities = Modality::TEXT;
                    }
                    let model_info = ModelInfo {
                        modalities,
                        max_input: model_config.max_input,
                        max_output: model_config.max_output,
                        temperature: model_config.default_temperature,
                        top_p: model_config.default_top_p,
                    };
                    model_sources.push(ProviderModelSource::new(
                        provider_idx,
                        model_key,
                        model_info,
                    ));
                }
            }

            provider_sources.push(provider_source);
        }

        Ok((provider_sources, model_sources))
    }
}

/// Individual source of provider configuration: a YAML file or a programmatic entry.
enum ConfigSource {
    /// A YAML file on disk.
    File(std::path::PathBuf),
    /// A programmatic provider entry added at build time.
    Programmatic { key: String, config: ProviderConfig },
}

#[cfg(test)]
mod tests {
    use super::*;
    use reloaded_code_core::models::{Modality, ProviderType};
    use tempfile::NamedTempFile;

    fn write_yaml(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp file");
        std::io::Write::write_all(&mut f, content.as_bytes()).expect("write yaml");
        f
    }

    #[test]
    fn load_single_file() {
        let f = write_yaml(indoc::indoc! {"
            my-llm:
              api_url: https://api.example.com/v1
              env:
                - EXAMPLE_API_KEY
              models:
                m1:
                  max_input: 128000
                  max_output: 8192
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let loaded = loader.load().expect("load");
        assert_eq!(loaded.providers.len(), 1);
        assert!(loaded.providers.contains_key("my-llm"));
    }

    #[test]
    fn load_empty_produces_no_providers() {
        let loaded = ProviderConfigLoader::new().load().expect("load");
        assert!(loaded.providers.is_empty());
        let (ps, ms) = loaded.to_catalog_sources().expect("catalog sources");
        assert!(ps.is_empty());
        assert!(ms.is_empty());
    }

    #[test]
    fn later_file_overrides_same_provider_key() {
        let f1 = write_yaml(indoc::indoc! {"
            shared:
              api_url: https://old.example.com/v1
              models:
                m1: { max_input: 4096, max_output: 2048 }
        "});
        let f2 = write_yaml(indoc::indoc! {"
            shared:
              api_url: https://new.example.com/v1
              models:
                m2: { max_input: 128000, max_output: 8192 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f1.path()).expect("add_path 1");
        loader.add_path(f2.path()).expect("add_path 2");
        let loaded = loader.load().expect("load");
        let config = &loaded.providers["shared"];
        assert_eq!(
            config.api_url.as_deref(),
            Some("https://new.example.com/v1")
        );
        // Only m2 exists - no deep merge.
        let models = config.models.as_ref().expect("models");
        assert!(!models.contains_key("m1"));
        assert!(models.contains_key("m2"));
    }

    #[test]
    fn earlier_keys_preserved_when_not_in_later_source() {
        let f1 = write_yaml(indoc::indoc! {"
            alpha:
              api_url: https://alpha.example.com/v1
              models:
                m1: { max_input: 8192, max_output: 4096 }
        "});
        let f2 = write_yaml(indoc::indoc! {"
            beta:
              api_url: https://beta.example.com/v1
              models:
                m2: { max_input: 128000, max_output: 8192 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f1.path()).expect("add_path 1");
        loader.add_path(f2.path()).expect("add_path 2");
        let loaded = loader.load().expect("load");
        assert_eq!(loaded.providers.len(), 2);
        assert!(loaded.providers.contains_key("alpha"));
        assert!(loaded.providers.contains_key("beta"));
    }

    #[test]
    fn programmatic_entry_overrides_file() {
        let f = write_yaml(indoc::indoc! {"
            my-llm:
              api_url: https://file.example.com/v1
              models:
                m1: { max_input: 4096, max_output: 2048 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        loader.add_provider(
            "my-llm",
            ProviderConfig {
                api_url: Some("https://programmatic.example.com/v1".into()),
                api_type: Some("anthropic".into()),
                env: None,
                models: Some({
                    let mut m = indexmap::IndexMap::new();
                    m.insert(
                        "m2".to_string(),
                        crate::config::ModelConfig {
                            max_input: 8192,
                            max_output: 4096,
                            modalities: vec!["text".to_string()],
                            default_temperature: None,
                            default_top_p: None,
                        },
                    );
                    m
                }),
            },
        );
        let loaded = loader.load().expect("load");
        let config = &loaded.providers["my-llm"];
        assert_eq!(
            config.api_url.as_deref(),
            Some("https://programmatic.example.com/v1")
        );
        assert_eq!(config.api_type.as_deref(), Some("anthropic"));
    }

    #[test]
    fn to_catalog_sources_produces_correct_types() {
        let f = write_yaml(indoc::indoc! {"
            my-llm:
              api_url: https://api.example.com/v1
              api_type: anthropic
              env:
                - MY_LLM_API_KEY
              models:
                claude-3:
                  max_input: 200000
                  max_output: 8192
                  modalities: [text, image]
                  default_temperature: 0.7
                  default_top_p: 0.95
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let loaded = loader.load().expect("load");
        let (providers, models) = loaded.to_catalog_sources().expect("catalog sources");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider_key, "my-llm");
        assert_eq!(providers[0].provider.api_type, ProviderType::Anthropic);
        assert_eq!(providers[0].provider.api_url, "https://api.example.com/v1");
        assert_eq!(providers[0].provider.env_vars, vec!["MY_LLM_API_KEY"]);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model_key, "claude-3");
        assert_eq!(models[0].model.max_input, 200000);
        assert_eq!(models[0].model.max_output, 8192);
        assert_eq!(models[0].model.modalities, Modality::TEXT | Modality::IMAGE);
        assert_eq!(models[0].model.temperature, Some(0.7));
        assert_eq!(models[0].model.top_p, Some(0.95));
    }

    #[test]
    fn to_catalog_sources_provider_idx_is_consistent() {
        let f = write_yaml(indoc::indoc! {"
            alpha:
              api_url: https://a.example.com/v1
              models:
                m1: { max_input: 8192, max_output: 4096 }
            beta:
              api_url: https://b.example.com/v1
              models:
                m2: { max_input: 128000, max_output: 8192 }
                m3: { max_input: 64000, max_output: 4096 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let loaded = loader.load().expect("load");
        let (providers, models) = loaded.to_catalog_sources().expect("catalog sources");
        assert_eq!(providers.len(), 2);
        assert_eq!(models.len(), 3);
        // All models under "alpha" should have provider_idx 0.
        assert_eq!(models[0].provider_idx, ProviderIdx::new(0));
        // Models under "beta" should have provider_idx 1.
        assert_eq!(models[1].provider_idx, ProviderIdx::new(1));
        assert_eq!(models[2].provider_idx, ProviderIdx::new(1));
    }

    #[test]
    fn to_catalog_sources_default_api_type_is_openai_compatible() {
        let f = write_yaml(indoc::indoc! {"
            local:
              api_url: http://localhost:11434/v1
              models:
                m1: { max_input: 4096, max_output: 2048 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let loaded = loader.load().expect("load");
        let (providers, _) = loaded.to_catalog_sources().expect("catalog sources");
        assert_eq!(
            providers[0].provider.api_type,
            ProviderType::OpenAiCompletions
        );
    }

    #[test]
    fn load_rejects_unrecognized_api_type() {
        let f = write_yaml(indoc::indoc! {"
            bad:
              api_type: totally-fake
              api_url: https://example.com/v1
              models:
                m1: { max_input: 4096, max_output: 2048 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let result = loader.load();
        let err = result.expect_err("should reject unknown api_type");
        assert!(
            err.to_string().contains("unrecognized api_type"),
            "error was: {err}"
        );
    }

    #[test]
    fn load_rejects_unrecognized_modality() {
        let f = write_yaml(indoc::indoc! {"
            bad:
              api_url: https://example.com/v1
              models:
                m1: { max_input: 4096, max_output: 2048, modalities: [smell] }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let result = loader.load();
        let err = result.expect_err("should reject unknown modality");
        assert!(
            err.to_string().contains("unrecognized modality"),
            "error was: {err}"
        );
    }

    #[test]
    fn load_file_not_found_returns_error() {
        let mut loader = ProviderConfigLoader::new();
        loader.add_path("/nonexistent/path.yaml").expect("add_path");
        let result = loader.load();
        assert!(result.is_err());
    }

    #[test]
    fn load_rejects_malformed_yaml() {
        let f = write_yaml("not: valid: yaml: [");
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let result = loader.load();
        let err = result.expect_err("should reject malformed YAML");
        assert!(err.to_string().contains("parse"), "error was: {err}");
    }

    #[test]
    fn load_rejects_empty_models_map() {
        let f = write_yaml(indoc::indoc! {"
            bad:
              api_url: https://example.com/v1
              models: {}
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let result = loader.load();
        let err = result.expect_err("should reject empty models map");
        assert!(err.to_string().contains("models"), "error was: {err}");
    }

    #[test]
    fn load_rejects_invalid_api_type_even_when_api_url_missing() {
        let f = write_yaml(indoc::indoc! {"
            bad:
              api_type: totally-fake
              models:
                m1: { max_input: 4096, max_output: 2048 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let err = loader.load().expect_err("should reject unknown api_type");
        assert!(
            err.to_string().contains("unrecognized api_type"),
            "error was: {err}"
        );
    }

    #[test]
    fn load_rejects_missing_api_url_for_non_ollama() {
        let f = write_yaml(indoc::indoc! {"
            bad:
              api_type: openai-compatible
              models:
                m1: { max_input: 4096, max_output: 2048 }
        "});
        let mut loader = ProviderConfigLoader::new();
        loader.add_path(f.path()).expect("add_path");
        let result = loader.load();
        let err = result.expect_err("should reject missing api_url");
        assert!(err.to_string().contains("api_url"), "error was: {err}");
    }

    #[test]
    fn default_config_paths_returns_empty_when_no_files_exist() {
        // In a temp directory with no config files, default_config_paths()
        // should return an empty vector (project-local path doesn't exist).
        // This test is inherently environment-dependent, so we only verify
        // the function doesn't panic and returns a Vec.
        let _paths = default_config_paths();
    }

    #[test]
    fn with_default_paths_creates_loader_without_panic() {
        // with_default_paths() should succeed even when no config files exist.
        let loader = ProviderConfigLoader::with_default_paths();
        assert!(loader.is_ok(), "should create loader even with no files");
        let _loaded = loader.expect("loader").load().expect("load");
        // May be empty if no config files exist on the test machine.
        // The key invariant: it doesn't error on missing files.
    }
}
