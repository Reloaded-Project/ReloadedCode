//! Efficient catalog/registry of providers and models sourced
//! from places like models.dev. Contains the bare minimum of information
//! required at runtime.
//!
//! For instance, a fully-qualified entry like
//! `synthetic/hf:moonshotai/Kimi-K2.5` can be split into:
//!
//! Provider: `synthetic`
//! Model: `hf:moonshotai/Kimi-K2.5`
//!
//! Internally the providers and models are stored in separate hash tables,
//! with friendly APIs to return them separately or combined when needed:
//!
//! - `ProviderTable`: `hash(provider_key) -> provider_idx`
//! - `ProviderModelTable`: `hash(provider_key + 0xFF + model_key) -> model_config_idx`
//!
//! The `ProviderModelTable` hashes the provider key and model key together,
//! separated by `0xFF`, so the same model name can exist under multiple
//! providers without ambiguity.
//!
//! Model metadata/config rows remain deduplicated in side tables, so repeated
//! configurations do not inflate memory usage.
//!
//! # Public API
//!
//! ## Building a Catalog
//!
//! - [`ModelCatalog::build`] - Batch builder entry point
//! - [`ProviderSource`] - Provider key + metadata input
//! - [`ProviderModelSource`] - Model key + metadata input for a provider
//! - [`ModelInfo`] - Model metadata input (modalities, token limits, sampling)
//! - [`ProviderInfo`] - Provider metadata input (API URL, env vars, type)
//! - [`Modality`] - Content modality flags (text, image, audio, video)
//!
//! ## Querying a Catalog
//!
//! - [`ModelCatalog::lookup_provider`] - Provider-only lookup
//! - [`ModelCatalog::lookup_provider_model`] - Provider-scoped model lookup
//! - [`ModelCatalog::lookup`] - Combined provider + model lookup
//! - [`ModelCatalog::provider_count`] - Total providers
//! - [`ModelCatalog::providers`] - Iterate all providers
//! - [`ModelCatalog::provider_model_count`] - Total provider-model entries
//! - [`ModelCatalog::model_config_count`] - Unique deduplicated model configs
//! - [`Provider`] / [`Model`] - Lookup return types
//!
//! ## Error Handling
//!
//! - [`ModelCatalogBuildError`] - Errors during catalog construction
//! - [`LookupTableKind`] - Identifies which lookup table had a collision
//!
//! # Why split provider and model tables?
//!
//! There's 2 reasons:
//!
//! 1. Many providers share the same models. A model like
//!    `moonshotai/Kimi-K2.5` may exist under multiple providers while still
//!    having provider specific metdata (e.g. different context lengths)
//!
//! 2. There's few providers (96), many provider-models (3,031).
//!    Storing both would require a 16-byte HashTable entry.
//!    But storing separately, we can have 8-byte entries.
//!    This saves 8 bytes on 2,935 entries (3,031-96). Or 23.48KB.
//!
//! Lookups are also infrequent, and users typically switch between models from
//! the same provider within a session. That means the provider usually needs to
//! be resolved only once per session.
//!
//! In the common case, switching models therefore adds little lookup overhead,
//! making the memory savings worth the extra indirection.
//!
//! # Extra Memory Optimizations
//!
//! To save memory, we do not store the original provider or model strings in
//! the catalog. The usual use case is that a caller already has a provider and
//! model ID, such as `synthetic/hf:moonshotai/Kimi-K2.5`, and just needs to
//! pull up metadata for it.
//!
//! Because `ModelCatalog` is usually built once at startup, we can reject
//! collisions and retry with new seeds if needed.
//!
//! ## Hash Collision Probabilities
//!
//! `ProviderTable` and `ProviderModelTable` use 48 bits from the 64-bit hash.
//!
//! Collision estimates use the birthday-bound approximation described by
//! [Preshing](https://preshing.com/20110504/hash-collision-probabilities/):
//!
//! `p(at least one collision) ~= 1 - exp(-n * (n - 1) / (2 * 2^48))`
//!
//! where `n` is the number of inserted keys.
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
//! - `ProviderTable`: 96 entries, 48-bit hash -> about `1 in 62 billion`
//! - `ProviderModelTable`: 3,031 entries, 48-bit hash -> about `1 in 61 million`
//!
//! Note: Above assumes a 'perfect' hash function with uniformly distributed output.
//!       While such function does not exist in practice, 'ahash' which I used
//!       here has very good distribution and comes fairly close.
//!
//! ## Reseeding
//!
//! As an additional safety measure, reseeding is also supported by trying
//! alternative hash seeds, up to 16 attempts.
//!
//! `ProviderTable` (96 entries, 48-bit):
//!
//! | Seeds | Odds of failure      |
//! | ----- | -------------------: |
//! | 1     | 1 in 62 billion      |
//! | 2     | 1 in 3.8 sextillion  |
//! | 4     | 1 in 1.5 x 10^43     |
//! | 8     | 1 in 2.1 x 10^86     |
//! | 16    | 1 in 4.4 x 10^172    |
//!
//! `ProviderModelTable` (3,031 entries, 48-bit):
//!
//! | Seeds | Odds of failure      |
//! | ----- | -------------------: |
//! | 1     | 1 in 61 million      |
//! | 2     | 1 in 3.8 quadrillion |
//! | 4     | 1 in 1.4 x 10^31     |
//! | 8     | 1 in 2.0 x 10^62     |
//! | 16    | 1 in 4.0 x 10^124    |
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
//! | Max provider env vars     |      16,384 | Global env-var pool offset (14-bit)              |
//! | Max env vars per provider |           3 | Count field in provider range entry (2-bit)      |
//! | Max input tokens          | 536,870,911 | 29-bit packed field (≈536M)                      |
//! | Max output tokens         | 134,217,727 | 27-bit packed field (≈134M)                      |
//! | Hash bits retained        |          48 | Truncated from 64-bit hash output                |
//! | Max reseed attempts       |          16 | Number of alternative hash seeds                 |
//!
//! # Detailed Memory Layout
//!
//! This layout is optimized for scenarios where many providers host overlapping
//! model configurations.
//!
//! Numbers below are from a models.dev snapshot.
//!
//! ## Statistics (models.dev snapshot example)
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
//! | Field                  | Type                                         | Size | Count |   Total  |
//! | ---------------------- | -------------------------------------------- | ---- | ----- | -------: |
//! | `provider_table`       | `HashTable<PackedProviderTableEntry>`        | 8 B  |    96 |    768 B |
//! | `provider_model_table` | `HashTable<PackedProviderModelTableEntry>`   | 8 B  | 3,031 | 24,248 B |
//! | `provider_entries`     | `Box<[ProviderType]>`                        | 1 B  |    96 |     96 B |
//! | `model_entries`        | `Box<[PackedModelEntry]>`                    | 8 B  |   585 |  4,680 B |
//! | `provider_env_ranges`  | `Box<[PackedEnvRange]>`                      | 2 B  |    96 |    192 B |
//!
//! **Packed metadata total: ~30.0 KB**
//!
//! ## Optional Metadata
//!
//! The `model_config_entries` field stores preset sampling parameters (`temperature`,
//! `top_p`) as [`ModelConfigEntry`] (4 bytes each). models.dev does not provide
//! this so we count this as 0.
//!
//! | Field                  | Type                              | Size | Count | Total |
//! | ---------------------- | --------------------------------- | ---- | ----- | ----: |
//! | `model_config_entries` | `Option<Box<[ModelConfigEntry]>>` | 4 B  |     0 |    -  |
//!
//! Alternative model info sources may provide recommended values for these fields.
//!
//! ## String Table Storage
//!
//! Provider API URLs and env-var names are stored in a compact buffer using
//! `lite_strtab`. 4GB max size.
//!
//! | Field               | Type                            | String Data | Offsets |   Total  |
//! | ------------------- | ------------------------------- | ----------: | ------: | -------: |
//! | `provider_api_urls` | `StringTable<u32, ProviderIdx>` |    2,460 B  |   296 B |  2,756 B |
//! | `provider_env_keys` | `StringTable<u32, ProviderIdx>` |    1,904 B  |   436 B |  2,340 B |
//!
//! **String tables total: ~5.1 KB** (null-terminated strings + 4-byte offsets)
//!
//! ## Other Runtime State
//!
//! | Field        | Type          | Size |
//! | ------------ | ------------- | ---: |
//! | `hash_state` | `RandomState` | ~8 B |
//!
//! ## Deduplication
//!
//! `ProviderModelTable` keys point to shared `model_entries` and optional
//! `model_config_entries` rows. If multiple provider models share the same
//! model metadata, the metadata is stored once and reused by index.

use crate::internal::hash64::Hash64;
use crate::models::ProviderType;
use ahash::RandomState;
use hashbrown::HashTable;
use internal::{
    build_from_source, hash_provider_key, hash_provider_model_key, Fixed4, ModelConfigEntry,
    PackedEnvRange, PackedModelEntry, PackedProviderModelTableEntry, PackedProviderTableEntry,
};
use lite_strtab::{StringId, StringTable};

pub use public::builder_types::{ModelCatalogBuildError, ProviderModelSource, ProviderSource};
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
    provider_model_table: HashTable<PackedProviderModelTableEntry>,
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
        provider_model_table: HashTable<PackedProviderModelTableEntry>,
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
            provider_model_table,
            provider_api_urls,
            provider_env_keys,
            provider_env_ranges,
            provider_entries,
            model_entries,
            model_config_entries,
        }
    }

    /// Builds a catalog from provider sources and provider model sources.
    ///
    /// # Parameters
    ///
    /// * `providers` - [`ProviderSource`] values keyed by provider identifier.
    /// * `provider_models` - [`ProviderModelSource`] values keyed by provider and model.
    ///
    /// # Errors
    ///
    /// Returns [`ModelCatalogBuildError`] when:
    /// - input exceeds supported numeric limits,
    /// - token limits cannot be represented in packed model entries,
    /// - provider model sources reference unknown providers,
    /// - or all seed-retry attempts still result in collisions.
    #[inline]
    pub fn build(
        providers: &[ProviderSource],
        provider_models: &[ProviderModelSource],
    ) -> Result<Self, ModelCatalogBuildError> {
        build_from_source(providers, provider_models)
    }

    /// Returns the number of providers in the catalog.
    ///
    /// # Returns
    ///
    /// The total number of provider entries.
    #[inline]
    pub fn provider_count(&self) -> usize {
        self.provider_table.len()
    }

    /// Iterates all providers in source insertion order.
    ///
    /// # Returns
    ///
    /// An iterator of [`Provider`] values.
    #[inline]
    pub fn providers(&self) -> impl Iterator<Item = Provider<'_>> + '_ {
        (0..self.provider_entries.len())
            .map(|idx| ProviderIdx::new(idx as u16))
            .filter_map(|provider_idx| self.provider_from_index(provider_idx))
    }

    /// Returns the number of provider-model entries in the catalog.
    ///
    /// This counts provider-specific `(provider_key, model_key)` entries before
    /// deduplicating shared model configurations, so it is always greater than
    /// or equal to [`Self::model_config_count`].
    ///
    /// For example, if providers `evroc`, `togetherai`, and `moonshotai` all
    /// expose `moonshotai/Kimi-K2.5`, this returns `3`.
    ///
    /// # Returns
    ///
    /// The total number of provider-model entries.
    #[inline]
    pub fn provider_model_count(&self) -> usize {
        self.provider_model_table.len()
    }

    /// Returns the number of unique model configurations in the catalog.
    ///
    /// Shared model metadata is deduplicated across provider-model entries, so
    /// this is always less than or equal to [`Self::provider_model_count`].
    ///
    /// For example, if providers `evroc`, `togetherai`, and `moonshotai` all
    /// expose `moonshotai/Kimi-K2.5` with identical metadata, this returns `1`.
    ///
    /// # Returns
    ///
    /// The number of unique model configurations.
    #[inline]
    pub fn model_config_count(&self) -> usize {
        self.model_entries.len()
    }

    /// Returns `true` when the catalog has no providers and no models.
    ///
    /// # Returns
    ///
    /// `true` if both provider and model tables are empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.provider_table.is_empty() && self.provider_model_table.is_empty()
    }

    /// Looks up one provider.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - Provider identifier (for example, `"openai"`).
    ///
    /// # Returns
    ///
    /// A [`Provider`] if the requested provider exists, otherwise `None`.
    #[inline]
    pub fn lookup_provider(&self, provider_key: &str) -> Option<Provider<'_>> {
        self.lookup_provider_hash(hash_provider_key(&self.hash_state, provider_key))
    }

    /// Looks up one model for a specific provider.
    ///
    /// This performs exact per-provider model lookup. The `model_key` must
    /// exist under the specified `provider_key`; the same model name under
    /// another provider does not match.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - Provider identifier (for example, `"openai"`).
    /// * `model_key` - Model identifier within that provider.
    ///
    /// # Returns
    ///
    /// A [`Model`] if the requested provider and model combination exists,
    /// otherwise `None`.
    #[inline]
    pub fn lookup_provider_model(&self, provider_key: &str, model_key: &str) -> Option<Model> {
        self.lookup_provider_model_hash(hash_provider_model_key(
            &self.hash_state,
            provider_key,
            model_key,
        ))
    }

    /// Looks up both provider and model and returns them as a pair.
    ///
    /// This performs exact per-provider model lookup. The `model_key` must
    /// exist under the specified `provider_key`; the same model name under
    /// another provider does not match.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - Provider identifier (for example, `"openai"`).
    /// * `model_key` - Model identifier within that provider.
    ///
    /// # Returns
    ///
    /// A `(`[`Provider`], [`Model`]`)` pair if the requested provider and model
    /// combination exists, otherwise `None`.
    #[inline]
    pub fn lookup(&self, provider_key: &str, model_key: &str) -> Option<(Provider<'_>, Model)> {
        Some((
            self.lookup_provider(provider_key)?,
            self.lookup_provider_model(provider_key, model_key)?,
        ))
    }

    /// Looks up one provider by prehashed key.
    #[inline]
    fn lookup_provider_hash(&self, key: Hash64) -> Option<Provider<'_>> {
        let hash48 = PackedProviderTableEntry::truncate_hash48(key.as_u64());
        let entry = self
            .provider_table
            .find(hash48, |entry: &PackedProviderTableEntry| {
                entry.hash48() == hash48
            })?;
        self.provider_from_index(entry.provider_idx_val())
    }

    /// Looks up one model for a specific provider by prehashed key.
    #[inline]
    fn lookup_provider_model_hash(&self, hash: Hash64) -> Option<Model> {
        let hash48 = PackedProviderModelTableEntry::truncate_hash48(hash.as_u64());
        let entry = self
            .provider_model_table
            .find(hash48, |entry: &PackedProviderModelTableEntry| {
                entry.hash48() == hash48
            })?;
        self.model_from_index(entry.model_config_idx_val())
    }

    /// Looks up a provider by its index.
    ///
    /// # Parameters
    ///
    /// * `provider_idx` - Provider index obtained from a previous lookup.
    ///
    /// # Returns
    ///
    /// The provider if `provider_idx` is in range, otherwise `None`.
    #[inline]
    pub fn provider_from_index(&self, provider_idx: ProviderIdx) -> Option<Provider<'_>> {
        let provider_idx_usize = provider_idx.as_usize();
        let api_type = *self.provider_entries.get(provider_idx_usize)?;
        let api_url = self.provider_api_urls.get(StringId::new(provider_idx))?;
        let range = self.provider_env_ranges.get(provider_idx_usize)?;
        let start = range.start();
        let count = range.count() as usize;

        let mut env_vars = ["", "", ""];
        #[allow(clippy::needless_range_loop)]
        for x in 0..count {
            env_vars[x] = self
                .provider_env_keys
                .get(StringId::new(ProviderIdx::new(start + x as u16)))?;
        }

        Some(Provider::new(
            provider_idx,
            api_url,
            env_vars,
            count as u8,
            api_type,
        ))
    }

    /// Looks up a model by its configuration index.
    #[inline]
    fn model_from_index(&self, model_config_idx: ModelIdx) -> Option<Model> {
        let idx = model_config_idx.as_usize();
        let info = self.model_entries.get(idx)?.into_model_info();
        let (temperature_fixed, top_p_fixed) = self
            .model_config_entries
            .as_ref()
            .and_then(|entries| entries.get(idx))
            .map(|entry| (entry.temperature_fixed(), entry.top_p_fixed()))
            .unwrap_or_else(|| {
                let none = Fixed4::from_encoded(Fixed4::NONE_SENTINEL);
                (none, none)
            });

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
        Modality, ModelInfo, ProviderInfo, ProviderModelSource, ProviderSource,
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
        provider_models: Vec<(&str, &str, ModelInfo)>,
    ) -> ModelCatalog {
        let provider_sources: Vec<ProviderSource> = providers
            .into_iter()
            .map(|(key, info)| ProviderSource::new(key, info))
            .collect();
        let provider_model_sources: Vec<ProviderModelSource> = provider_models
            .into_iter()
            .map(|(provider_key, model_key, info)| {
                ProviderModelSource::new(provider_key, model_key, info)
            })
            .collect();
        ModelCatalog::build(&provider_sources, &provider_model_sources)
            .expect("build catalog from source rows")
    }

    #[test]
    fn lookup_is_provider_model_specific() {
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
                ("alpha", "m1", info(8192, 1024)),
                ("beta", "m1", info(16_384, 2_048)),
            ],
        );

        let alpha_provider = catalog.lookup_provider("alpha").expect("alpha exists");
        let beta_provider = catalog.lookup_provider("beta").expect("beta exists");
        let alpha_model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 exists");
        let beta_model = catalog
            .lookup_provider_model("beta", "m1")
            .expect("beta/m1 exists");
        let (alpha_provider_lookup, alpha_lookup_model) =
            catalog.lookup("alpha", "m1").expect("alpha/m1 exists");
        let (beta_provider_lookup, beta_lookup_model) =
            catalog.lookup("beta", "m1").expect("beta/m1 exists");

        assert_eq!(alpha_provider.api_url, "https://alpha.example");
        assert_eq!(alpha_provider.api_type, ProviderType::OpenAiCompletions);
        assert_eq!(alpha_provider.env_vars(), &["ALPHA_KEY"]);
        assert_eq!(beta_provider.api_url, "https://beta.example");
        assert_eq!(beta_provider.api_type, ProviderType::Azure);
        assert_eq!(beta_provider.env_vars(), &["BETA_KEY"]);
        assert_eq!(alpha_model.max_output, 1024);
        assert_eq!(beta_model.max_output, 2_048);
        assert_eq!(alpha_provider_lookup.api_url, "https://alpha.example");
        assert_eq!(alpha_provider_lookup.env_vars(), &["ALPHA_KEY"]);
        assert_eq!(alpha_lookup_model.max_output, 1024);
        assert_eq!(beta_provider_lookup.api_url, "https://beta.example");
        assert_eq!(beta_provider_lookup.env_vars(), &["BETA_KEY"]);
        assert_eq!(beta_lookup_model.max_output, 2_048);
    }

    #[test]
    fn missing_provider_model_edge_returns_none() {
        let catalog = build_catalog(
            vec![
                (
                    "alpha",
                    provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
                ),
                (
                    "beta",
                    provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
                ),
            ],
            vec![
                ("alpha", "m1", info(4096, 512)),
                ("beta", "m2", info(8192, 1024)),
            ],
        );

        assert!(catalog.lookup_provider("missing").is_none());
        assert!(catalog.lookup_provider_model("alpha", "m2").is_none());
        assert!(catalog.lookup_provider_model("beta", "m1").is_none());
        assert!(catalog.lookup_provider_model("missing", "m2").is_none());
        assert!(catalog.lookup("alpha", "m2").is_none());
        assert!(catalog.lookup("beta", "m1").is_none());
        assert!(catalog.lookup("missing", "m2").is_none());
    }

    #[test]
    fn model_entries_are_deduplicated_by_info_and_config() {
        let catalog = build_catalog(
            vec![
                (
                    "alpha",
                    provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
                ),
                (
                    "beta",
                    provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
                ),
            ],
            vec![
                ("alpha", "m1", info_with_sampling(4096, 512, 1.0, 0.9)),
                ("beta", "m2", info_with_sampling(4096, 512, 1.0, 0.9)),
            ],
        );

        assert_eq!(catalog.provider_model_count(), 2);
        assert_eq!(catalog.model_config_count(), 1);
    }

    #[test]
    fn provider_count_matches_provider_iterator() {
        let catalog = build_catalog(
            vec![
                (
                    "alpha",
                    provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
                ),
                (
                    "beta",
                    provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
                ),
            ],
            vec![
                ("alpha", "m1", info(4096, 512)),
                ("beta", "m2", info(4096, 512)),
            ],
        );

        let providers: Vec<Provider<'_>> = catalog.providers().collect();
        assert_eq!(catalog.provider_count(), providers.len());
        assert_eq!(providers[0].api_url, "https://alpha.example");
        assert_eq!(providers[1].api_url, "https://beta.example");
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
            vec![("alpha", "m1", info(4096, 512))],
        );

        let (_, model) = catalog.lookup("alpha", "m1").expect("lookup exists");
        assert_eq!(model.temperature(), None);
        assert_eq!(model.top_p(), None);
    }
}
