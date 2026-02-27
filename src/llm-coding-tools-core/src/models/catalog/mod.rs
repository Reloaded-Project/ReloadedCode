//! Efficient catalog/registry of providers and models sourced
//! from places like 'models.dev'. Contains bare minimum of information
//! required for usage.
//!
//! For instance; a model entry like `synthetic/hf:moonshotai/Kimi-K2.5` may be
//! split into:
//!
//! Provider: 'synthetic'
//! Model: 'hf:moonshotai/Kimi-K2.5'
//!
//! Internally the `provider`(s) and `model`(s) are stored in separate tables;
//! with friendly APIs to return those back combined when needed.
//!
//! # Public API
//!
//! ## Building a Catalog
//!
//! - [`ModelCatalog::build`] - Batch builder entry point
//! - [`ProviderSourceRow`] - Provider key + metadata input row
//! - [`ModelSourceRow`] - Model key + metadata input row
//! - [`ModelInfo`] - Model metadata input (modalities, token limits, sampling)
//! - [`ProviderInfo`] - Provider metadata input (API URL, env vars, type)
//! - [`Modality`] - Content modality flags (text, image, audio, video)
//!
//! ## Querying a Catalog
//!
//! - [`ModelCatalog`] - Immutable lookup catalog
//! - [`Model`] - Model lookup result
//! - [`Provider`] - Provider lookup result
//! - [`CatalogEntry`] - Combined provider + model lookup result
//!
//! ## Error Handling
//!
//! - [`ModelCatalogBuildError`] - Errors during catalog construction
//! - [`LookupTableKind`] - Identifies which hash table had a collision
//!
//! # Why split provider and model tables?
//!
//! Many providers share the same models. Although they may sometimes be renamed,
//! e.g. `Kimi-K2.5` vs `hf:moonshotai/Kimi-K2.5`; they often have identical
//! metadata. (token limits, modalities, etc.)
//!
//! Given a snapshot of models.dev from 20th of February 2026, we have:
//!
//! - Unique model IDs: 1,669
//! - Unique model configurations: 552
//!
//! We optimize for this case hashtables of `hash -> index` 😉
//!
//! # Memory Optimizations
//!
//! To save on memory, we don't actually store the original strings for provider
//! or model anywhere. The typical use case is that a user has a given provider &
//! model ID e.g. `synthetic/hf:moonshotai/Kimi-K2.5` and just needs to pull
//! up metadata for it. e.g. when `model` is specified in an agent file.
//!
//! Instead, we provide a guarantee that a *VALID* user provided provider and
//! model key will always hash to unique values (0 collisions). Since the
//! `ModelCatalog` is usually constructed once at startup, this is something
//! we can practically guarantee. (negligible failure probability)
//!
//! Sometimes this concept is referred to as a 'perfect hash', elsewhere.
//!
//! ## Hash Collision Probabilities
//!
//! Currently `ProviderTable` and `ModelTable` use 48 bits for the hash.
//!
//! | Odds of collision | # 48-bit hash values |
//! | ----------------- | -------------------: |
//! | 1 in 2            |           19,753,663 |
//! | 1 in 10           |            7,701,474 |
//! | 1 in 100          |            2,378,621 |
//! | 1 in 1,000        |              750,488 |
//! | 1 in 10,000       |              237,272 |
//! | 1 in 100,000      |               75,031 |
//! | 1 in 1 million    |               23,727 |
//! | 1 in 10 million   |                7,503 |
//! | 1 in 100 million  |                2,373 |
//! | 1 in 1 billion    |                  751 |
//! | 1 in 10 billion   |                  238 |
//! | 1 in 100 billion  |                   76 |
//! | 1 in 1 trillion   |                   24 |
//! | 1 in 10 trillion  |                    8 |
//!
//! Today's probabilities of 'at least 1 collision' are:
//!
//! - `ProviderTable`: 96 entries, 48-bit hash -> about `1 in 61 billion`
//! - `ModelTable`: 1,669 entries, 48-bit hash -> about `1 in 202 million`
//!
//! Note: Above assumes a 'perfect' hash function with uniformly distributed output.
//!       While such function does not exist in practice, 'ahash' which I used
//!       here has very good distribution and comes fairly close.
//!
//! ## Reseeding
//!
//! As an additional safety measure, re-seeding is also supported.
//! i.e. Using alternative seeds for hashing.
//!
//! ProviderTable (96 entries, 48-bit):
//!
//! | Seeds | Odds of failure      |
//! | ----- | -------------------: |
//! | 1     | 1 in 62 billion      |
//! | 2     | 1 in 3.8 quintillion |
//! | 4     | 1 in 1.4 x 10^43     |
//! | 8     | 1 in 2.1 x 10^86     |
//! | 16    | 1 in 4.4 x 10^172    |
//!
//! ModelTable (1,669 entries, 48-bit):
//!
//! | Seeds | Odds of failure      |
//! | ----- | -------------------: |
//! | 1     | 1 in 202 million     |
//! | 2     | 1 in 41 quadrillion  |
//! | 4     | 1 in 1.7 x 10^33     |
//! | 8     | 1 in 2.8 x 10^66     |
//! | 16    | 1 in 7.8 x 10^132    |
//!
//! This basically seals the deal, ensuring a collision will never happen.
//!
//! As a point of reference, there are estimated to be 10^78 to 10^82 atoms in
//! the observable universe.
//!
//! # Numeric Limits
//!
//! | Limit                     |       Value | Description                                      |
//! | ------------------------- | ----------: | ------------------------------------------------ |
//! | Max providers             |      65,536 | Addressable by 16-bit provider index             |
//! | Max model configs         |      65,536 | Addressable by 16-bit model configuration index  |
//! | Max provider env vars     |      16,384 | Per-provider env-var pool offset (14-bit)        |
//! | Max env vars per provider |           3 | Count field in provider entry (2-bit)            |
//! | Max input tokens          | 536,870,911 | 29-bit packed field (≈536M)                      |
//! | Max output tokens         | 134,217,727 | 27-bit packed field (≈134M)                      |
//! | Hash bits retained        |          48 | Truncated from 64-bit ahash output               |
//! | Max reseed attempts       |          16 | Number of alternative hash seeds                 |
//!
//! Note: There's technically 16 bits per provider, but only 14 bits for provider env var.
//! Since each provider typically has 1 env var; that means 14 bits for provider, effectively.
//!
//! # Detailed Memory Layout
//!
//! This layout is optimized for scenarios where many providers host overlapping
//! models. Numbers below are from real API data (`api.json`):
//!
//! ## Statistics
//!
//! | Metric                               | Value   |
//! | ------------------------------------ | ------: |
//! | Unique providers                     |      96 |
//! | Total model entries                  |   3,031 |
//! | Unique model configurations          |     585 |
//! | Avg models sharing same config       |    5.18 |
//!
//! ## Packed Metadata Storage
//!
//! | Field                 | Type                                  | Size | Count |   Total  |
//! | --------------------- | ------------------------------------- | ---- | ----- | -------: |
//! | `provider_table`      | `HashTable<PackedProviderTableEntry>` | 8 B  |    96 |    768 B |
//! | `model_table`         | `HashTable<PackedModelTableEntry>`    | 8 B  | 3,031 | 24,248 B |
//! | `provider_entries`    | `Box<[ProviderType]>`                 | 1 B  |    96 |     96 B |
//! | `model_entries`       | `Box<[PackedModelEntry]>`             | 8 B  |   585 |  4,680 B |
//! | `provider_env_ranges` | `Box<[PackedEnvRange]>`               | 2 B  |    96 |    192 B |
//!
//! **Packed metadata total: ~26.0 KB**
//!
//! ## Optional Metadata
//!
//! The `model_config_entries` field stores preset sampling parameters (`temperature`,
//! `top_p`) as [`ModelConfigEntry`] (4 bytes each). models.dev does not provide
//! this so this is currently markes as `None`.
//!
//! | Field                  | Type                               | Size | Count | Total |
//! | ---------------------- | ---------------------------------- | ---- | ----- | ----: |
//! | `model_config_entries` | `Option<Box<[ModelConfigEntry]>>`  | 4 B  |     0 |    —  |
//!
//! ## String Table Storage
//!
//! | Field               | Type                           | String Data | Offsets |   Total  |
//! | ------------------- | ------------------------------ | ----------: | ------: | -------: |
//! | `provider_api_urls` | `StringTable<u32, ProviderIdx>`|    2,460 B  |   296 B |  2,756 B |
//! | `provider_env_keys` | `StringTable<u32, ProviderIdx>`|    1,904 B  |   436 B |  2,340 B |
//!
//! **String tables total: ~5.1 KB** (null-terminated strings + 4-byte offsets)
//!
//! ## Other Runtime State
//!
//! | Field        | Type          | Size |
//! | ------------ | ------------- | ---- |
//! | `hash_state` | `RandomState` | ~8 B |
//!
//! String tables use `lite_strtab` with 4-byte offsets.
//!
//! ## Deduplication
//!
//! The key insight is that `ModelTable` keys can point to shared
//! `ModelEntry` / `ModelConfigEntry` rows. When multiple providers host the
//! same model, we only store the metadata once. This is why we have 1,669
//! model keys but only 552 unique model configurations.

use crate::models::ProviderType;
use ahash::RandomState;
use hashbrown::HashTable;
use internal::{
    build_from_source, hash_model_key, hash_provider_key, Fixed4, ModelConfigEntry, PackedEnvRange,
    PackedModelEntry, PackedModelTableEntry, PackedProviderTableEntry, ProviderHash,
};
use lite_strtab::{StringId, StringTable};

pub use public::builder_types::{ModelCatalogBuildError, ModelSourceRow, ProviderSourceRow};
pub use public::*;

mod internal;
mod public;

/// Runtime lookup catalog with split provider and model tables.
///
/// See module-level documentation for design rationale, memory layout,
/// and numeric limits.
pub struct ModelCatalog {
    hash_state: RandomState,
    provider_table: HashTable<PackedProviderTableEntry>,
    model_table: HashTable<PackedModelTableEntry>,
    provider_api_urls: StringTable<u32, ProviderIdx>,
    provider_env_keys: StringTable<u32, ProviderIdx>,
    provider_env_ranges: Box<[PackedEnvRange]>,
    provider_entries: Box<[ProviderType]>,
    model_entries: Box<[PackedModelEntry]>,
    model_config_entries: Option<Box<[ModelConfigEntry]>>,
}

impl ModelCatalog {
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        hash_state: RandomState,
        provider_table: HashTable<PackedProviderTableEntry>,
        model_table: HashTable<PackedModelTableEntry>,
        provider_api_urls: StringTable<u32, ProviderIdx>,
        provider_env_keys: StringTable<u32, ProviderIdx>,
        provider_env_ranges: Box<[PackedEnvRange]>,
        provider_entries: Box<[ProviderType]>,
        model_entries: Box<[PackedModelEntry]>,
        model_config_entries: Option<Box<[ModelConfigEntry]>>,
    ) -> Self {
        Self {
            hash_state,
            provider_table,
            model_table,
            provider_api_urls,
            provider_env_keys,
            provider_env_ranges,
            provider_entries,
            model_entries,
            model_config_entries,
        }
    }

    /// Builds a catalog from provider and model source rows.
    ///
    /// # Parameters
    ///
    /// * `providers` - [`ProviderSourceRow`] values keyed by provider identifier.
    /// * `models` - [`ModelSourceRow`] values keyed by model identifier.
    ///
    /// # Returns
    ///
    /// A fully built [`ModelCatalog`] when construction succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogBuildError`] when:
    /// - input exceeds supported numeric limits,
    /// - token limits cannot be represented in packed model entries,
    /// - or all seed-retry attempts still result in collisions.
    #[inline]
    pub fn build(
        providers: &[ProviderSourceRow],
        models: &[ModelSourceRow],
    ) -> Result<Self, ModelCatalogBuildError> {
        build_from_source(providers, models)
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
    fn lookup_model_hash(&self, hash: internal::hash::ModelHash) -> Option<Model> {
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
            model
                .temperature()
                .and_then(Fixed4::from_f32)
                .unwrap_or_else(|| Fixed4::from_encoded(Fixed4::NONE_SENTINEL)),
            model
                .top_p()
                .and_then(Fixed4::from_f32)
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

        let temperature_fixed = temperature
            .and_then(Fixed4::from_f32)
            .unwrap_or_else(|| Fixed4::from_encoded(Fixed4::NONE_SENTINEL));
        let top_p_fixed = top_p
            .and_then(Fixed4::from_f32)
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
    use crate::models::catalog::{
        Modality, ModelInfo, ModelSourceRow, ProviderInfo, ProviderSourceRow,
    };

    fn provider(api_url: &str, env_vars: &[&str], api_type: ProviderType) -> ProviderInfo {
        ProviderInfo {
            api_url: api_url.to_owned(),
            env_vars: env_vars.iter().map(|s| s.to_string()).collect(),
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

    fn build_catalog(
        providers: Vec<(&str, ProviderInfo)>,
        models: Vec<(&str, ModelInfo)>,
    ) -> ModelCatalog {
        let provider_rows: Vec<ProviderSourceRow> = providers
            .into_iter()
            .map(|(key, info)| ProviderSourceRow::new(key, info))
            .collect();
        let model_rows: Vec<ModelSourceRow> = models
            .into_iter()
            .map(|(key, info)| ModelSourceRow::new(key, info))
            .collect();

        ModelCatalog::build(&provider_rows, &model_rows).expect("build catalog from source rows")
    }

    #[test]
    fn lookup_provider_and_model_work_independently() {
        let catalog = build_catalog(
            vec![
                (
                    "alpha",
                    provider(
                        "https://alpha.example",
                        &["ALPHA_KEY"],
                        ProviderType::OpenAiCompletions,
                    ),
                ),
                (
                    "beta",
                    provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
                ),
            ],
            vec![
                ("m1", info_with_sampling(8192, 1024, 1.2, 0.5)),
                ("m2", info(16_384, 2_048)),
            ],
        );

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
        let catalog = build_catalog(
            vec![(
                "alpha",
                provider("", &["ALPHA_KEY"], ProviderType::OpenAiCompletions),
            )],
            vec![("m1", info(4096, 512))],
        );

        assert!(catalog.lookup_provider("missing").is_none());
        assert!(catalog.lookup_model("missing").is_none());
        assert!(catalog.lookup("missing", "m1").is_none());
        assert!(catalog.lookup("alpha", "missing").is_none());
    }

    #[test]
    fn model_entries_are_deduplicated_by_info_and_config() {
        let catalog = build_catalog(
            Vec::new(),
            vec![
                ("m1", info_with_sampling(4096, 512, 1.0, 0.9)),
                ("m2", info_with_sampling(4096, 512, 1.0, 0.9)),
            ],
        );

        assert_eq!(catalog.model_len(), 2);
        assert_eq!(catalog.model_config_len(), 1);
    }

    #[test]
    fn provider_env_vars_are_flattened_and_indexed() {
        let catalog = build_catalog(
            vec![(
                "azure",
                provider(
                    "https://azure.example",
                    &["AZURE_KEY", "AZURE_TOKEN", "FALLBACK_KEY"],
                    ProviderType::Azure,
                ),
            )],
            Vec::new(),
        );

        let provider = catalog
            .lookup_provider("azure")
            .expect("provider azure exists");
        assert_eq!(provider.env_vars.len(), 3);
        assert_eq!(provider.env_vars[0], "AZURE_KEY");
        assert_eq!(provider.env_vars[1], "AZURE_TOKEN");
        assert_eq!(provider.env_vars[2], "FALLBACK_KEY");
    }

    #[test]
    fn none_temperature_and_top_p_use_none_sentinel() {
        let catalog = build_catalog(
            vec![(
                "alpha",
                provider(
                    "https://alpha.example",
                    &["KEY"],
                    ProviderType::OpenAiCompletions,
                ),
            )],
            vec![("m1", info(4096, 512))],
        );

        let m1 = catalog.lookup_model("m1").expect("model m1 exists");
        assert_eq!(m1.temperature(), None);
        assert_eq!(m1.top_p(), None);

        let joined = catalog.lookup("alpha", "m1").expect("joined lookup exists");
        assert_eq!(joined.temperature(), None);
        assert_eq!(joined.top_p(), None);
    }
}
