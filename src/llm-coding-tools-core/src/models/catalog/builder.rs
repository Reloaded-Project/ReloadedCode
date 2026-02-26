use crate::models::catalog::internal::{
    hash_model_key, hash_provider_key, hash_state_for_seed, model_table_entry_hash,
    provider_table_entry_hash, ModelConfigEntry, PackedEnvRange, PackedModelEntry,
    PackedModelTableEntry, PackedProviderTableEntry, MAX_INPUT_TOKENS, MAX_MODEL_CONFIG_COUNT,
    MAX_OUTPUT_TOKENS, MAX_PROVIDER_COUNT,
};
use crate::models::catalog::public::builder_types::{
    LookupTableKind, ModelCatalogBuildError, ModelInfo, ProviderInfo,
};
use crate::models::catalog::public::ProviderIdx;
use crate::models::catalog::ModelCatalog;
use crate::models::ProviderType;
use ahash::AHashMap;
use hashbrown::HashTable;
use lite_strtab::{Global, StringTable, StringTableBuilder};

/// Maximum hash seed value.
///
/// This is the upper limit for reseeding attempts when hash collisions occur.
/// Using u8::MAX allows for 256 different hash seeds (0-255).
pub const MAX_SEED: u8 = u8::MAX;

/// Incremental builder for [`ModelCatalog`].
///
/// Providers and models are inserted independently so caller-side key formats
/// remain implementation-specific.
#[derive(Debug, Clone)]
pub struct ModelCatalogBuilder {
    seed: u8,
    hash_state: ahash::RandomState,
    provider_table: HashTable<PackedProviderTableEntry>,
    model_table: HashTable<PackedModelTableEntry>,
    provider_api_urls: Vec<String>,
    provider_env_keys: Vec<String>,
    provider_env_ranges: Vec<PackedEnvRange>,
    provider_entries: Vec<ProviderType>,
    model_entries: Vec<PackedModelEntry>,
    model_config_entries: Vec<ModelConfigEntry>,
    model_entry_intern: AHashMap<(PackedModelEntry, ModelConfigEntry), u16>,
    has_any_model_config: bool,
}

impl ModelCatalogBuilder {
    /// Creates a builder with no preallocated capacity.
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(0, 0)
    }

    /// Creates a builder with preallocated provider and model key capacity.
    #[inline]
    pub fn with_capacity(provider_capacity: usize, model_capacity: usize) -> Self {
        Self {
            seed: 0,
            hash_state: hash_state_for_seed(0),
            provider_table: HashTable::with_capacity(provider_capacity),
            model_table: HashTable::with_capacity(model_capacity),
            provider_api_urls: Vec::with_capacity(provider_capacity),
            provider_env_keys: Vec::new(),
            provider_env_ranges: Vec::with_capacity(provider_capacity),
            provider_entries: Vec::with_capacity(provider_capacity),
            model_entries: Vec::with_capacity(model_capacity),
            model_config_entries: Vec::with_capacity(model_capacity),
            model_entry_intern: AHashMap::with_capacity(model_capacity),
            has_any_model_config: false,
        }
    }

    /// Returns the currently selected hash seed.
    #[inline]
    pub const fn seed(&self) -> u8 {
        self.seed
    }

    /// Returns the number of inserted providers.
    ///
    /// # Returns
    ///
    /// The total number of provider entries inserted into the builder.
    #[inline]
    pub fn provider_len(&self) -> usize {
        self.provider_table.len()
    }

    /// Returns the total number of inserted model keys.
    ///
    /// This includes all model entries before deduplication. Multiple keys may
    /// reference the same configuration (see [`Self::model_config_len`]).
    ///
    /// For example, inserting `moonshotai/Kimi-K2.5` under providers `evroc`,
    /// `togetherai`, and `moonshotai` with identical metadata, this returns 3.
    ///
    /// Note: Model key names depend on the source. For models.dev, they follow
    /// the `{owner}/{model}` format, but other registries may use different naming.
    ///
    /// # Returns
    ///
    /// The total number of model entries inserted into the builder.
    #[inline]
    pub fn model_len(&self) -> usize {
        self.model_table.len()
    }

    /// Returns the number of unique model configurations.
    ///
    /// Models with identical metadata are deduplicated and share a configuration
    /// entry. This is always less than or equal to [`Self::model_len`].
    ///
    /// For example, inserting `moonshotai/Kimi-K2.5` under providers `evroc`,
    /// `togetherai`, and `moonshotai` with identical metadata, this returns 1.
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

    /// Returns true when no providers and no models are inserted.
    ///
    /// # Returns
    ///
    /// `true` if both provider and model tables are empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.provider_table.is_empty() && self.model_table.is_empty()
    }

    /// Reserves capacity for additional provider keys.
    ///
    /// Preallocates internal storage to avoid reallocations when inserting
    /// multiple providers.
    ///
    /// # Parameters
    ///
    /// * `additional` - The number of additional providers expected to be inserted.
    #[inline]
    pub fn reserve_providers(&mut self, additional: usize) {
        self.provider_table
            .reserve(additional, provider_table_entry_hash);
        self.provider_api_urls.reserve(additional);
        self.provider_env_ranges.reserve(additional);
        self.provider_entries.reserve(additional);
    }

    /// Reserves capacity for additional model keys.
    ///
    /// Preallocates internal storage to avoid reallocations when inserting
    /// multiple models.
    ///
    /// # Parameters
    ///
    /// * `additional` - The number of additional models expected to be inserted.
    #[inline]
    pub fn reserve_models(&mut self, additional: usize) {
        self.model_table.reserve(additional, model_table_entry_hash);
        self.model_entries.reserve(additional);
        self.model_config_entries.reserve(additional);
        self.model_entry_intern.reserve(additional);
    }

    /// Inserts a provider entry into the catalog.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - The unique provider identifier (e.g., `"openai"`, `"moonshotai"`).
    /// * `info` - Provider metadata including API URL, environment variables, and type.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the provider was inserted successfully.
    /// * `Err(ModelCatalogBuildError::HashCollision)` if a hash collision is detected
    ///   for the current seed. Call [`Self::reset`] to try with a new seed.
    /// * `Err(ModelCatalogBuildError::TooManyProviders)` if the maximum provider count
    ///   is exceeded.
    /// * `Err(ModelCatalogBuildError::TooManyProviderEnvVarsForOneProvider)` if the
    ///   provider has too many environment variables.
    #[inline]
    pub fn insert_provider(
        &mut self,
        provider_key: &str,
        info: &ProviderInfo<'_>,
    ) -> Result<(), ModelCatalogBuildError> {
        use crate::models::catalog::internal::MAX_ENV_RANGE_COUNT;

        if self.provider_entries.len() >= MAX_PROVIDER_COUNT {
            return Err(ModelCatalogBuildError::TooManyProviders {
                count: self.provider_entries.len() + 1,
                max: MAX_PROVIDER_COUNT,
            });
        }

        let env_count = info.env_vars.len();
        if env_count > usize::from(MAX_ENV_RANGE_COUNT) {
            return Err(
                ModelCatalogBuildError::TooManyProviderEnvVarsForOneProvider {
                    count: env_count,
                    max: usize::from(MAX_ENV_RANGE_COUNT),
                },
            );
        }

        let key = hash_provider_key(&self.hash_state, provider_key);
        let hash48 = PackedProviderTableEntry::truncate_hash48(key.as_u64());
        if self
            .provider_table
            .find(hash48, |existing: &PackedProviderTableEntry| {
                existing.hash48() == hash48
            })
            .is_some()
        {
            return Err(ModelCatalogBuildError::HashCollision {
                table: LookupTableKind::Provider,
                seed: self.seed,
            });
        }

        let provider_idx = self.provider_entries.len() as u16;
        let env_start = self.provider_env_keys.len() as u16;
        let env_count = env_count as u8;
        // Store API URL
        self.provider_api_urls.push(info.api_url.to_owned());

        // Store env keys and range
        for &var in info.env_vars {
            self.provider_env_keys.push(var.to_owned());
        }
        self.provider_env_ranges
            .push(PackedEnvRange::from_parts(env_start, env_count));

        self.provider_entries.push(info.api_type);
        self.provider_table.insert_unique(
            hash48,
            PackedProviderTableEntry::from_parts(key.as_u64(), provider_idx),
            provider_table_entry_hash,
        );

        Ok(())
    }

    /// Inserts a model entry into the catalog.
    ///
    /// Models with identical metadata are automatically deduplicated and share
    /// a single configuration entry.
    ///
    /// # Parameters
    ///
    /// * `model_key` - The unique model identifier (e.g., `"gpt-4"`, `"moonshotai/Kimi-K2.5"`).
    ///   Note that model key format depends on the source registry.
    /// * `info` - Model metadata including token limits, modalities, and optional sampling defaults.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the model was inserted successfully.
    /// * `Err(ModelCatalogBuildError::HashCollision)` if a hash collision is detected
    ///   for the current seed. Call [`Self::reset`] to try with a new seed.
    /// * `Err(ModelCatalogBuildError::TooManyModelConfigurations)` if the maximum
    ///   unique configuration count is exceeded.
    /// * `Err(ModelCatalogBuildError::MaxInputTokensOutOfRange)` if max_input exceeds
    ///   the packed limit.
    /// * `Err(ModelCatalogBuildError::MaxOutputTokensOutOfRange)` if max_output exceeds
    ///   the packed limit.
    #[inline]
    pub fn insert_model(
        &mut self,
        model_key: &str,
        info: ModelInfo,
    ) -> Result<(), ModelCatalogBuildError> {
        if info.max_output > MAX_OUTPUT_TOKENS {
            return Err(ModelCatalogBuildError::MaxOutputTokensOutOfRange {
                max_output: info.max_output,
                max: MAX_OUTPUT_TOKENS,
            });
        }

        if info.max_input > MAX_INPUT_TOKENS {
            return Err(ModelCatalogBuildError::MaxInputTokensOutOfRange {
                max_input: info.max_input,
                max: MAX_INPUT_TOKENS,
            });
        }

        let model_entry = PackedModelEntry::from_model_info(info);
        let config_entry = ModelConfigEntry::from_sampling(info.temperature, info.top_p);
        if !config_entry.is_none() {
            self.has_any_model_config = true;
        }

        let model_config_idx = match self.model_entry_intern.get(&(model_entry, config_entry)) {
            Some(existing) => *existing,
            None => {
                if self.model_entries.len() >= MAX_MODEL_CONFIG_COUNT {
                    return Err(ModelCatalogBuildError::TooManyModelConfigurations {
                        count: self.model_entries.len() + 1,
                        max: MAX_MODEL_CONFIG_COUNT,
                    });
                }

                let next_idx = self.model_entries.len() as u16;
                self.model_entries.push(model_entry);
                self.model_config_entries.push(config_entry);
                self.model_entry_intern
                    .insert((model_entry, config_entry), next_idx);
                next_idx
            }
        };

        let key = hash_model_key(&self.hash_state, model_key);
        let hash48 = PackedModelTableEntry::truncate_hash48(key.as_u64());
        if self
            .model_table
            .find(hash48, |existing: &PackedModelTableEntry| {
                existing.hash48() == hash48
            })
            .is_some()
        {
            return Err(ModelCatalogBuildError::HashCollision {
                table: LookupTableKind::Model,
                seed: self.seed,
            });
        }

        self.model_table.insert_unique(
            hash48,
            PackedModelTableEntry::from_parts(key.as_u64(), model_config_idx),
            model_table_entry_hash,
        );

        Ok(())
    }

    /// Resets the builder to handle hash collisions.
    ///
    /// Clears all inserted entries and advances to the next hash seed.
    /// Capacity is retained so callers can replay inserts without reallocating.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if reset succeeded and the seed was advanced.
    /// * `Err(ModelCatalogBuildError::HashCollisionExhausted)` if all seeds have
    ///   been exhausted.
    #[inline]
    pub fn reset(&mut self) -> Result<(), ModelCatalogBuildError> {
        if self.seed == MAX_SEED {
            return Err(ModelCatalogBuildError::HashCollisionExhausted {
                attempts: MAX_SEED.into(),
            });
        }

        self.seed = self.seed.wrapping_add(1);
        self.hash_state = hash_state_for_seed(self.seed);
        self.provider_table.clear();
        self.model_table.clear();
        self.provider_api_urls.clear();
        self.provider_env_keys.clear();
        self.provider_env_ranges.clear();
        self.provider_entries.clear();
        self.model_entries.clear();
        self.model_config_entries.clear();
        self.model_entry_intern.clear();
        self.has_any_model_config = false;

        Ok(())
    }

    /// Finalizes the builder into a [`ModelCatalog`].
    ///
    /// Consumes the builder and returns an immutable catalog ready for lookups.
    ///
    /// # Returns
    ///
    /// A finalized [`ModelCatalog`] containing all inserted providers and models.
    #[inline]
    pub fn build(self) -> ModelCatalog {
        let model_config_entries = if self.has_any_model_config {
            Some(self.model_config_entries.into_boxed_slice())
        } else {
            None
        };

        // Build StringTables from accumulated strings
        let api_url_table = build_string_table(&self.provider_api_urls);
        let env_keys_table = build_string_table(&self.provider_env_keys);

        ModelCatalog {
            hash_state: self.hash_state,
            provider_table: self.provider_table,
            model_table: self.model_table,
            provider_api_urls: api_url_table,
            provider_env_keys: env_keys_table,
            provider_env_ranges: self.provider_env_ranges.into_boxed_slice(),
            provider_entries: self.provider_entries.into_boxed_slice(),
            model_entries: self.model_entries.into_boxed_slice(),
            model_config_entries,
        }
    }
}

/// Builds a StringTable from a slice of strings.
#[inline]
fn build_string_table(strings: &[String]) -> StringTable<u32, ProviderIdx> {
    let total_bytes: usize = strings.iter().map(|s| s.len()).sum();
    let mut builder = StringTableBuilder::<u32, ProviderIdx>::with_capacity_in(
        strings.len(),
        total_bytes,
        Global,
    );
    for s in strings {
        builder.try_push(s).expect("string table insert");
    }
    builder.build()
}

impl Default for ModelCatalogBuilder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ModelCatalogBuilder;
    use crate::models::catalog::{
        LookupTableKind, Modality, ModelCatalogBuildError, ModelInfo, ProviderInfo,
    };
    use crate::models::ProviderType;

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

    #[test]
    fn collisions_report_table_kind_and_seed() {
        let mut builder = ModelCatalogBuilder::new();
        builder
            .insert_provider("alpha", &provider("", &[], ProviderType::OpenAiCompletions))
            .expect("first insert succeeds");

        let err = builder
            .insert_provider("alpha", &provider("", &[], ProviderType::OpenAiCompletions))
            .expect_err("duplicate hash should fail");
        assert_eq!(
            err,
            ModelCatalogBuildError::HashCollision {
                table: LookupTableKind::Provider,
                seed: 0,
            }
        );
    }

    #[test]
    fn reset_advances_seed_and_clears_tables() {
        let mut builder = ModelCatalogBuilder::new();
        builder
            .insert_model("m1", info(4096, 512))
            .expect("insert model");
        assert_eq!(builder.seed(), 0);

        builder.reset().expect("reset should advance seed");
        assert_eq!(builder.seed(), 1);
        assert!(builder.is_empty());
    }

    #[test]
    fn reset_exhaustion_returns_error_at_seed_limit() {
        let mut builder = ModelCatalogBuilder::new();

        for _ in 0..super::MAX_SEED {
            builder.reset().expect("reset within seed range must work");
        }

        let err = builder
            .reset()
            .expect_err("reset should fail after all seeds are consumed");
        assert_eq!(
            err,
            ModelCatalogBuildError::HashCollisionExhausted {
                attempts: super::MAX_SEED.into()
            }
        );
    }

    #[test]
    fn max_output_tokens_out_of_range_returns_error() {
        let mut builder = ModelCatalogBuilder::new();
        let max_output = super::MAX_OUTPUT_TOKENS;

        let err = builder
            .insert_model("m1", info(4096, max_output.saturating_add(1)))
            .expect_err("max output over packed limit should fail");

        assert_eq!(
            err,
            ModelCatalogBuildError::MaxOutputTokensOutOfRange {
                max_output: max_output.saturating_add(1),
                max: max_output,
            }
        );
    }

    #[test]
    fn max_input_tokens_out_of_range_returns_error() {
        let mut builder = ModelCatalogBuilder::new();
        let max_input = super::MAX_INPUT_TOKENS;

        let err = builder
            .insert_model("m1", info(max_input.saturating_add(1), 512))
            .expect_err("max input over packed limit should fail");

        assert_eq!(
            err,
            ModelCatalogBuildError::MaxInputTokensOutOfRange {
                max_input: max_input.saturating_add(1),
                max: max_input,
            }
        );
    }
}
