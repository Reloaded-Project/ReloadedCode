use crate::models::catalog::builder::ModelCatalogBuilder;
use crate::models::catalog::internal::{
    hash_model_key, hash_provider_key, Fixed4, ModelConfigEntry, PackedEnvRange, PackedModelEntry,
    PackedModelTableEntry, PackedProviderTableEntry, ProviderHash,
};
use crate::models::catalog::public::{CatalogEntry, Model, ModelIdx, Provider, ProviderIdx};
use crate::models::ProviderType;
use ahash::RandomState;
use hashbrown::HashTable;
use lite_strtab::{StringId, StringTable};

/// Runtime lookup catalog with split provider and model tables.
///
/// See module-level documentation for design rationale, memory layout,
/// and numeric limits.
pub struct ModelCatalog {
    /// Precomputed hash state for the selected seed.
    pub(crate) hash_state: RandomState,
    /// Provider key lookup table.
    pub(crate) provider_table: HashTable<PackedProviderTableEntry>,
    /// Model key lookup table.
    pub(crate) model_table: HashTable<PackedModelTableEntry>,
    /// Provider API URLs indexed by provider index.
    pub(crate) provider_api_urls: StringTable<u32, ProviderIdx>,
    /// Provider env keys grouped in a string table.
    pub(crate) provider_env_keys: StringTable<u32, ProviderIdx>,
    /// Env key ranges (start, count) indexed by provider index.
    pub(crate) provider_env_ranges: Box<[PackedEnvRange]>,
    /// Provider types indexed by provider index.
    pub(crate) provider_entries: Box<[ProviderType]>,
    /// Packed deduplicated model metadata indexed by model-configuration index.
    pub(crate) model_entries: Box<[PackedModelEntry]>,
    /// Optional model sampling sidecars indexed by model-configuration index.
    pub(crate) model_config_entries: Option<Box<[ModelConfigEntry]>>,
}

impl ModelCatalog {
    /// Creates a builder with no preallocated capacity.
    #[inline]
    pub fn builder() -> ModelCatalogBuilder {
        ModelCatalogBuilder::new()
    }

    /// Creates a builder with preallocated provider and model key capacity.
    #[inline]
    pub fn builder_with_capacity(
        provider_capacity: usize,
        model_capacity: usize,
    ) -> ModelCatalogBuilder {
        ModelCatalogBuilder::with_capacity(provider_capacity, model_capacity)
    }

    /// Returns the number of provider keys.
    ///
    /// # Returns
    ///
    /// The total number of provider entries in the catalog.
    #[inline]
    pub fn provider_len(&self) -> usize {
        self.provider_table.len()
    }

    /// Returns the total number of model keys.
    ///
    /// This includes all model entries before deduplication. Multiple keys may
    /// reference the same configuration (see [`Self::model_config_len`]).
    ///
    /// For example, if providers `evroc`, `togetherai`, and `moonshotai` all
    /// host `moonshotai/Kimi-K2.5` with identical metadata, this returns 3.
    ///
    /// Note: Model key names depend on the source. For models.dev, they follow
    /// the `{owner}/{model}` format, but other registries may use different naming.
    ///
    /// # Returns
    ///
    /// The total number of model entries in the catalog.
    #[inline]
    pub fn model_len(&self) -> usize {
        self.model_table.len()
    }

    /// Returns the number of unique model configurations.
    ///
    /// Models with identical metadata are deduplicated and share a configuration
    /// entry. This is always less than or equal to [`Self::model_len`].
    ///
    /// For example, if providers `evroc`, `togetherai`, and `moonshotai` all
    /// host `moonshotai/Kimi-K2.5` with identical metadata, this returns 1.
    ///
    /// Note: Model key names depend on the source. For models.dev, they follow
    /// the `{owner}/{model}` format, but other registries may use different naming.
    ///
    /// # Returns
    ///
    /// The number of unique model configuration rows.
    #[inline]
    pub fn model_config_len(&self) -> usize {
        self.model_entries.len()
    }

    /// Returns true when catalog has no providers and no models.
    ///
    /// # Returns
    ///
    /// `true` if both provider and model tables are empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.provider_table.is_empty() && self.model_table.is_empty()
    }

    /// Looks up a provider by its key.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - The provider identifier (e.g., `"openai"`, `"moonshotai"`).
    ///
    /// # Returns
    ///
    /// The provider information if found, or `None` if not present.
    #[inline]
    pub fn lookup_provider(&self, provider_key: &str) -> Option<Provider<'_>> {
        let key = hash_provider_key(&self.hash_state, provider_key);
        self.lookup_provider_hash(key)
    }

    /// Looks up one provider by prehashed key.
    #[inline]
    fn lookup_provider_hash(&self, key: ProviderHash) -> Option<Provider<'_>> {
        let hash48 = PackedProviderTableEntry::truncate_hash48(key.as_u64());
        let entry = self
            .provider_table
            .find(hash48, |entry: &PackedProviderTableEntry| {
                entry.hash48() == hash48
            })?;
        self.provider_from_index(entry.provider_idx_val())
    }

    /// Looks up a model by its key.
    ///
    /// # Parameters
    ///
    /// * `model_key` - The model identifier (e.g., `"gpt-4"`, `"moonshotai/Kimi-K2.5"`).
    ///   Note that model key format depends on the source registry.
    ///
    /// # Returns
    ///
    /// The model information if found, or `None` if not present.
    #[inline]
    pub fn lookup_model(&self, model_key: &str) -> Option<Model> {
        let hash = hash_model_key(&self.hash_state, model_key);
        self.lookup_model_hash(hash)
    }

    /// Looks up one model by prehashed key.
    #[inline]
    fn lookup_model_hash(
        &self,
        hash: crate::models::catalog::internal::hash::ModelHash,
    ) -> Option<Model> {
        let hash48 = PackedModelTableEntry::truncate_hash48(hash.as_u64());
        let entry = self
            .model_table
            .find(hash48, |entry: &PackedModelTableEntry| {
                entry.hash48() == hash48
            })?;
        self.model_from_index(entry.model_config_idx_val())
    }

    /// Looks up both provider and model and returns a combined entry.
    ///
    /// This is a convenience method that performs both lookups and combines
    /// the results into a single [`CatalogEntry`].
    ///
    /// # Parameters
    ///
    /// * `provider_key` - The provider identifier.
    /// * `model_key` - The model identifier.
    ///
    /// # Returns
    ///
    /// A combined provider and model entry if both are found, or `None` if
    /// either is missing.
    #[inline]
    pub fn lookup(&self, provider_key: &str, model_key: &str) -> Option<CatalogEntry<'_>> {
        let provider =
            self.lookup_provider_hash(hash_provider_key(&self.hash_state, provider_key))?;
        let model = self.lookup_model_hash(hash_model_key(&self.hash_state, model_key))?;

        Some(CatalogEntry::new(
            provider.provider_idx,
            provider.api_url,
            provider.env_vars,
            provider.api_type,
            model.model_config_idx,
            model.modalities,
            model.max_input,
            model.max_output,
            Fixed4::from_f32(model.temperature().unwrap_or(0.0))
                .unwrap_or_else(|| Fixed4::from_encoded(Fixed4::NONE_SENTINEL)),
            Fixed4::from_f32(model.top_p().unwrap_or(0.0))
                .unwrap_or_else(|| Fixed4::from_encoded(Fixed4::NONE_SENTINEL)),
        ))
    }

    /// Looks up a provider by its index.
    ///
    /// # Parameters
    ///
    /// * `provider_idx` - The provider index obtained from a previous lookup.
    ///
    /// # Returns
    ///
    /// The provider information if the index is valid, or `None` if out of bounds.
    #[inline]
    pub fn provider_from_index(&self, provider_idx: ProviderIdx) -> Option<Provider<'_>> {
        let provider_idx_usize = provider_idx.as_usize();
        let api_type = *self.provider_entries.get(provider_idx_usize)?;
        let api_url = self.provider_api_urls.get(StringId::new(provider_idx))?;
        let range = self.provider_env_ranges.get(provider_idx_usize)?;
        let start = range.start();
        let count = range.count();

        let env_vars: Vec<&str> = if count == 0 {
            Vec::new()
        } else {
            let mut vars = Vec::with_capacity(usize::from(count));
            for i in 0..count {
                let idx = ProviderIdx::new(start + u16::from(i));
                if let Some(s) = self.provider_env_keys.get(StringId::new(idx)) {
                    vars.push(s);
                }
            }
            vars
        };

        Some(Provider {
            provider_idx,
            api_url,
            env_vars,
            api_type,
        })
    }

    /// Looks up a model by its configuration index.
    ///
    /// # Parameters
    ///
    /// * `model_config_idx` - The model configuration index obtained from a previous lookup.
    ///
    /// # Returns
    ///
    /// The model information if the index is valid, or `None` if out of bounds.
    #[inline]
    pub fn model_from_index(&self, model_config_idx: ModelIdx) -> Option<Model> {
        let idx = model_config_idx.as_usize();
        let info = self.model_entries.get(idx)?.into_model_info();
        let (temperature, top_p) = self
            .model_config_entries
            .as_ref()
            .and_then(|entries| entries.get(idx))
            .map(|entry| (entry.temperature(), entry.top_p()))
            .unwrap_or((None, None));

        let temperature_fixed = Fixed4::from_f32(temperature.unwrap_or(0.0))
            .unwrap_or_else(|| Fixed4::from_encoded(Fixed4::NONE_SENTINEL));
        let top_p_fixed = Fixed4::from_f32(top_p.unwrap_or(0.0))
            .unwrap_or_else(|| Fixed4::from_encoded(Fixed4::NONE_SENTINEL));

        Some(Model::new(
            model_config_idx,
            info.modalities,
            info.max_input,
            info.max_output,
            temperature_fixed,
            top_p_fixed,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::catalog::{Modality, ModelInfo, ProviderInfo};

    fn provider<'a>(
        api_url: &'a str,
        env_vars: &'a [&'a str],
        api_type: ProviderType,
    ) -> ProviderInfo<'a> {
        ProviderInfo {
            api_url,
            env_vars,
            api_type,
        }
    }

    fn info(max_input: u32, max_output: u32) -> ModelInfo {
        ModelInfo {
            modalities: Modality::TEXT,
            max_input,
            max_output,
            temperature: None,
            top_p: None,
        }
    }

    fn info_with_sampling(
        max_input: u32,
        max_output: u32,
        temperature: f32,
        top_p: f32,
    ) -> ModelInfo {
        ModelInfo {
            modalities: Modality::TEXT,
            max_input,
            max_output,
            temperature: Some(temperature),
            top_p: Some(top_p),
        }
    }

    #[test]
    fn lookup_provider_and_model_work_independently() {
        let mut builder = ModelCatalog::builder_with_capacity(2, 2);
        builder
            .insert_provider(
                "alpha",
                &provider(
                    "https://alpha.example",
                    &["ALPHA_KEY"],
                    ProviderType::OpenAiCompletions,
                ),
            )
            .expect("insert provider alpha");
        builder
            .insert_provider(
                "beta",
                &provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
            )
            .expect("insert provider beta");

        builder
            .insert_model("m1", info_with_sampling(8192, 1024, 1.2, 0.5))
            .expect("insert model m1");
        builder
            .insert_model("m2", info(16_384, 2_048))
            .expect("insert model m2");

        let catalog = builder.build();
        let alpha = catalog
            .lookup_provider("alpha")
            .expect("provider alpha exists");
        assert_eq!(alpha.api_url, "https://alpha.example");
        assert_eq!(alpha.api_type, ProviderType::OpenAiCompletions);

        let m1 = catalog.lookup_model("m1").expect("model m1 exists");
        assert_eq!(m1.max_input, 8192);
        assert_eq!(m1.max_output, 1024);
        assert_eq!(m1.temperature(), Some(1.2));
        assert_eq!(m1.top_p(), Some(0.5));

        let joined = catalog.lookup("alpha", "m1").expect("joined lookup exists");
        assert_eq!(joined.api_url, "https://alpha.example");
        assert_eq!(joined.max_output, 1024);
    }

    #[test]
    fn unknown_provider_or_model_returns_none() {
        let mut builder = ModelCatalog::builder();
        builder
            .insert_provider(
                "alpha",
                &provider("", &["ALPHA_KEY"], ProviderType::OpenAiCompletions),
            )
            .expect("insert provider");
        builder
            .insert_model("m1", info(4096, 512))
            .expect("insert model");
        let catalog = builder.build();

        assert!(catalog.lookup_provider("missing").is_none());
        assert!(catalog.lookup_model("missing").is_none());
        assert!(catalog.lookup("missing", "m1").is_none());
        assert!(catalog.lookup("alpha", "missing").is_none());
    }

    #[test]
    fn model_entries_are_deduplicated_by_info_and_config() {
        let mut builder = ModelCatalog::builder();

        builder
            .insert_model("m1", info_with_sampling(4096, 512, 1.0, 0.9))
            .expect("insert m1");
        builder
            .insert_model("m2", info_with_sampling(4096, 512, 1.0, 0.9))
            .expect("insert m2");

        let catalog = builder.build();
        assert_eq!(catalog.model_len(), 2);
        assert_eq!(catalog.model_config_len(), 1);
    }

    #[test]
    fn provider_env_vars_are_flattened_and_indexed() {
        let mut builder = ModelCatalog::builder();
        builder
            .insert_provider(
                "azure",
                &provider(
                    "https://azure.example",
                    &["AZURE_KEY", "AZURE_TOKEN", "FALLBACK_KEY"],
                    ProviderType::Azure,
                ),
            )
            .expect("insert provider azure");

        let catalog = builder.build();
        let provider = catalog
            .lookup_provider("azure")
            .expect("provider azure exists");
        assert_eq!(provider.env_vars.len(), 3);
        assert_eq!(provider.env_vars[0], "AZURE_KEY");
        assert_eq!(provider.env_vars[1], "AZURE_TOKEN");
        assert_eq!(provider.env_vars[2], "FALLBACK_KEY");
    }
}
