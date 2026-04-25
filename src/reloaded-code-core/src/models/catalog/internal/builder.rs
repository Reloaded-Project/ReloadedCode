use crate::models::catalog::internal::{
    hash_provider_key, hash_provider_model_key, hash_state_for_seed,
    provider_model_table_entry_hash, provider_table_entry_hash, ModelConfigEntry, PackedEnvRange,
    PackedModelEntry, PackedProviderModelTableEntry, PackedProviderTableEntry, MAX_ENV_RANGE_COUNT,
    MAX_ENV_START, MAX_INPUT_TOKENS, MAX_MODEL_CONFIG_COUNT, MAX_OUTPUT_TOKENS, MAX_PROVIDER_COUNT,
};
use crate::models::catalog::public::builder_types::{
    LookupTableKind, ModelCatalogBuildError, ProviderModelSource, ProviderSource,
};
use crate::models::catalog::public::ProviderIdx;
use crate::models::catalog::ModelCatalog;
use crate::models::ProviderType;
use ahash::{AHashMap, AHashSet};
use hashbrown::{hash_table::Entry as TableEntry, HashTable};
use lite_strtab::{Global, StringTable, StringTableBuilder};
use std::collections::hash_map::Entry as MapEntry;

/// Maximum hash seed value.
///
/// This is the upper limit for reseeding attempts when hash collisions occur.
/// Using u8::MAX allows for 256 different hash seeds (0-255).
pub const MAX_SEED: u8 = u8::MAX;

#[derive(Debug, Clone, Copy)]
struct ProviderSourceStats {
    provider_count: usize,
    total_api_url_bytes: usize,
    total_env_keys: usize,
    total_env_key_bytes: usize,
}

#[derive(Debug, Clone)]
struct BuildState {
    seed: u8,
    hash_state: ahash::RandomState,
    provider_table: HashTable<PackedProviderTableEntry>,
    provider_model_table: HashTable<PackedProviderModelTableEntry>,
    provider_env_ranges: Vec<PackedEnvRange>,
    provider_entries: Vec<ProviderType>,
    model_entries: Vec<PackedModelEntry>,
    model_config_entries: Vec<ModelConfigEntry>,
    model_entry_intern: AHashMap<(PackedModelEntry, ModelConfigEntry), u16>,
    has_any_model_config: bool,
}

#[inline]
fn build_state_with_capacity(
    provider_capacity: usize,
    provider_model_capacity: usize,
) -> BuildState {
    BuildState {
        seed: 0,
        hash_state: hash_state_for_seed(0),
        provider_table: HashTable::with_capacity(provider_capacity),
        provider_model_table: HashTable::with_capacity(provider_model_capacity),
        provider_env_ranges: Vec::with_capacity(provider_capacity),
        provider_entries: Vec::with_capacity(provider_capacity),
        model_entries: Vec::with_capacity(provider_model_capacity),
        model_config_entries: Vec::with_capacity(provider_model_capacity),
        model_entry_intern: AHashMap::with_capacity(provider_model_capacity),
        has_any_model_config: false,
    }
}

/// Builds a catalog from provider and provider-model sources.
///
/// This is an internal construction path used by [`ModelCatalog::build`].
#[inline]
pub(crate) fn build_from_source(
    providers: &[ProviderSource],
    provider_models: &[ProviderModelSource<'_>],
) -> Result<ModelCatalog, ModelCatalogBuildError> {
    let provider_stats = analyze_provider_sources(providers)?;
    let mut state = build_state_with_capacity(provider_stats.provider_count, provider_models.len());

    loop {
        match populate_tables_once(&mut state, providers, provider_models) {
            Ok(()) => break,
            Err(ModelCatalogBuildError::HashCollision { .. }) => {
                advance_seed_and_clear(&mut state)?;
            }
            Err(err) => return Err(err),
        }
    }

    finish_with_source(state, providers, provider_stats)
}

#[inline]
fn populate_tables_once(
    state: &mut BuildState,
    providers: &[ProviderSource],
    provider_models: &[ProviderModelSource<'_>],
) -> Result<(), ModelCatalogBuildError> {
    let mut env_start: u16 = 0;
    let mut seen_provider_keys: AHashSet<&str> = AHashSet::with_capacity(providers.len());
    let mut seen_provider_models: AHashSet<(ProviderIdx, &str)> =
        AHashSet::with_capacity(provider_models.len());

    for provider in providers {
        let provider_info = &provider.provider;
        let env_count = provider_info.env_vars.len() as u8;

        if !seen_provider_keys.insert(provider.provider_key.as_str()) {
            return Err(ModelCatalogBuildError::DuplicateKey {
                table: LookupTableKind::Provider,
                key: provider.provider_key.clone(),
            });
        }

        insert_provider(
            state,
            &provider.provider_key,
            env_start,
            env_count,
            provider_info.api_type,
        )?;

        // SAFETY: analyze_provider_sources bounds env_start and env_count (<= 7).
        env_start += u16::from(env_count);
    }

    for provider_model in provider_models {
        let provider = providers
            .get(provider_model.provider_idx.as_usize())
            .ok_or(ModelCatalogBuildError::ProviderIdxOutOfRangeForModel {
                provider_idx: provider_model.provider_idx,
                model_key: provider_model.model_key.to_owned(),
            })?;

        // Check for duplicate (provider_idx, model_key) pair.
        let key = (provider_model.provider_idx, provider_model.model_key);
        if !seen_provider_models.insert(key) {
            return Err(ModelCatalogBuildError::DuplicateKey {
                table: LookupTableKind::ProviderModel,
                key: format!("{}/{}", provider.provider_key, provider_model.model_key),
            });
        }
        insert_provider_model(state, provider.provider_key.as_str(), provider_model)?;
    }

    Ok(())
}

#[inline]
fn insert_provider(
    state: &mut BuildState,
    provider_key: &str,
    env_start: u16,
    env_count: u8,
    api_type: ProviderType,
) -> Result<ProviderIdx, ModelCatalogBuildError> {
    let key = hash_provider_key(&state.hash_state, provider_key);
    let hash48 = PackedProviderTableEntry::truncate_hash48(key.as_u64());
    let provider_idx = ProviderIdx::new(state.provider_entries.len() as u16);

    match state.provider_table.entry(
        hash48,
        |existing: &PackedProviderTableEntry| existing.hash48() == hash48,
        provider_table_entry_hash,
    ) {
        TableEntry::Occupied(_) => {
            return Err(ModelCatalogBuildError::HashCollision {
                table: LookupTableKind::Provider,
                seed: state.seed,
            });
        }
        TableEntry::Vacant(vacant) => {
            vacant.insert(PackedProviderTableEntry::from_parts_idx(
                key.as_u64(),
                provider_idx,
            ));
        }
    }

    // Add env range and provider entry.
    state
        .provider_env_ranges
        .push(PackedEnvRange::from_parts(env_start, env_count));
    state.provider_entries.push(api_type);

    Ok(provider_idx)
}

#[inline]
fn insert_provider_model(
    state: &mut BuildState,
    provider_key: &str,
    provider_model: &ProviderModelSource<'_>,
) -> Result<(), ModelCatalogBuildError> {
    let info = provider_model.model;

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
    let config_entry = ModelConfigEntry::from_sampling(info.temperature, info.top_p)?;
    state.has_any_model_config |= !config_entry.is_none();

    // Deduplicate model config entries.
    let model_config_idx = match state.model_entry_intern.entry((model_entry, config_entry)) {
        MapEntry::Occupied(existing) => *existing.get(),
        MapEntry::Vacant(vacant) => {
            if state.model_entries.len() >= MAX_MODEL_CONFIG_COUNT {
                return Err(ModelCatalogBuildError::TooManyModelConfigurations {
                    count: state.model_entries.len() + 1,
                    max: MAX_MODEL_CONFIG_COUNT,
                });
            }

            let next_idx = state.model_entries.len() as u16;
            state.model_entries.push(model_entry);
            state.model_config_entries.push(config_entry);
            vacant.insert(next_idx);
            next_idx
        }
    };

    let key = hash_provider_model_key(&state.hash_state, provider_key, provider_model.model_key);
    let hash48 = PackedProviderModelTableEntry::truncate_hash48(key.as_u64());

    // Insert provider-model entry.
    match state.provider_model_table.entry(
        hash48,
        |existing: &PackedProviderModelTableEntry| existing.hash48() == hash48,
        provider_model_table_entry_hash,
    ) {
        TableEntry::Occupied(_) => {
            return Err(ModelCatalogBuildError::HashCollision {
                table: LookupTableKind::ProviderModel,
                seed: state.seed,
            });
        }
        TableEntry::Vacant(vacant) => {
            vacant.insert(PackedProviderModelTableEntry::from_parts(
                key.as_u64(),
                model_config_idx,
            ));
        }
    }

    Ok(())
}

#[inline]
fn advance_seed_and_clear(state: &mut BuildState) -> Result<(), ModelCatalogBuildError> {
    if state.seed == MAX_SEED {
        return Err(ModelCatalogBuildError::HashCollisionExhausted {
            attempts: MAX_SEED.into(),
        });
    }

    state.seed += 1;
    state.hash_state = hash_state_for_seed(state.seed);
    clear_entries(state);
    Ok(())
}

#[inline]
fn clear_entries(state: &mut BuildState) {
    state.provider_table.clear();
    state.provider_model_table.clear();
    state.provider_env_ranges.clear();
    state.provider_entries.clear();
    state.model_entries.clear();
    state.model_config_entries.clear();
    state.model_entry_intern.clear();
    state.has_any_model_config = false;
}

#[inline]
fn finish_with_source(
    mut state: BuildState,
    providers: &[ProviderSource],
    provider_stats: ProviderSourceStats,
) -> Result<ModelCatalog, ModelCatalogBuildError> {
    state
        .provider_table
        .shrink_to_fit(provider_table_entry_hash);
    state
        .provider_model_table
        .shrink_to_fit(provider_model_table_entry_hash);

    let model_config_entries = if state.has_any_model_config {
        Some(state.model_config_entries.into_boxed_slice())
    } else {
        None
    };

    Ok(ModelCatalog::new(
        state.hash_state,
        state.provider_table,
        state.provider_model_table,
        build_provider_api_url_table(providers, provider_stats)?,
        build_provider_env_key_table(providers, provider_stats)?,
        state.provider_env_ranges.into_boxed_slice(),
        state.provider_entries.into_boxed_slice(),
        state.model_entries.into_boxed_slice(),
        model_config_entries,
    ))
}

#[inline]
fn analyze_provider_sources(
    providers: &[ProviderSource],
) -> Result<ProviderSourceStats, ModelCatalogBuildError> {
    let provider_count = providers.len();
    if provider_count > MAX_PROVIDER_COUNT {
        return Err(ModelCatalogBuildError::TooManyProviders {
            count: provider_count,
            max: MAX_PROVIDER_COUNT,
        });
    }

    let mut total_api_url_bytes = 0usize;
    let mut total_env_keys = 0usize;
    let mut total_env_key_bytes = 0usize;
    let max_env_start = usize::from(MAX_ENV_START);
    let max_env_count = usize::from(MAX_ENV_RANGE_COUNT);

    for provider in providers {
        // SAFETY: total_env_keys is the start index for this provider.
        // It must fit the 13-bit PackedEnvRange start field.
        if total_env_keys > max_env_start {
            return Err(ModelCatalogBuildError::TooManyEnvVarKeys {
                count: total_env_keys,
                max: max_env_start,
            });
        }

        let provider_info = &provider.provider;
        let env_count = provider_info.env_vars.len();
        // SAFETY: per-provider count must fit the 3-bit count field.
        if env_count > max_env_count {
            return Err(
                ModelCatalogBuildError::TooManyProviderEnvVarsForOneProvider {
                    count: env_count,
                    max: max_env_count,
                },
            );
        }

        total_api_url_bytes += provider_info.api_url.len();
        total_env_keys += env_count;
        for env_key in &provider_info.env_vars {
            total_env_key_bytes += env_key.len();
        }
    }

    Ok(ProviderSourceStats {
        provider_count,
        total_api_url_bytes,
        total_env_keys,
        total_env_key_bytes,
    })
}

#[inline]
fn build_provider_api_url_table(
    providers: &[ProviderSource],
    stats: ProviderSourceStats,
) -> Result<StringTable<u32, ProviderIdx>, ModelCatalogBuildError> {
    let mut builder = StringTableBuilder::<u32, ProviderIdx>::with_capacity_in(
        stats.provider_count,
        stats.total_api_url_bytes,
        Global,
    );

    for provider in providers {
        builder
            .try_push(&provider.provider.api_url)
            .map_err(|e| ModelCatalogBuildError::StringTableCapacityExceeded(e.to_string()))?;
    }

    Ok(builder.build())
}

#[inline]
fn build_provider_env_key_table(
    providers: &[ProviderSource],
    stats: ProviderSourceStats,
) -> Result<StringTable<u32, ProviderIdx>, ModelCatalogBuildError> {
    let mut builder = StringTableBuilder::<u32, ProviderIdx>::with_capacity_in(
        stats.total_env_keys,
        stats.total_env_key_bytes,
        Global,
    );

    for provider in providers {
        for env_key in &provider.provider.env_vars {
            builder
                .try_push(env_key)
                .map_err(|e| ModelCatalogBuildError::StringTableCapacityExceeded(e.to_string()))?;
        }
    }

    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::build_from_source;
    use crate::models::catalog::{
        LookupTableKind, Modality, ModelCatalogBuildError, ModelInfo, ProviderIdx, ProviderInfo,
        ProviderModelSource, ProviderSource,
    };
    use crate::models::ProviderType;

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

    fn provider_source(provider_key: &str, provider: ProviderInfo) -> ProviderSource {
        ProviderSource::new(provider_key, provider)
    }

    fn provider_model_source<'a>(
        provider_idx: ProviderIdx,
        model_key: &'a str,
        model: ModelInfo,
    ) -> ProviderModelSource<'a> {
        ProviderModelSource::new(provider_idx, model_key, model)
    }

    fn test_sources() -> (Vec<ProviderSource>, Vec<ProviderModelSource<'static>>) {
        (
            vec![provider_source(
                "alpha",
                provider(
                    "https://alpha.example",
                    &["ALPHA_KEY"],
                    ProviderType::OpenAiCompletions,
                ),
            )],
            vec![provider_model_source(
                ProviderIdx::new(0),
                "m1",
                info(4096, 512),
            )],
        )
    }

    #[test]
    fn build_from_source_builds_catalog() {
        let (providers, provider_models) = test_sources();
        let catalog =
            build_from_source(&providers, &provider_models).expect("source build should succeed");

        assert_eq!(catalog.provider_count(), 1);
        assert_eq!(catalog.provider_model_count(), 1);
        assert!(catalog.lookup("alpha", "m1").is_some());
    }

    #[test]
    fn duplicate_provider_keys_returns_error() {
        let providers = vec![
            provider_source(
                "alpha",
                provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
            ),
            provider_source(
                "alpha",
                provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
            ),
        ];
        let provider_models = vec![provider_model_source(
            ProviderIdx::new(0),
            "m1",
            info(4096, 512),
        )];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::DuplicateKey {
                        table: LookupTableKind::Provider,
                        key: "alpha".to_string(),
                    }
                );
            }
            Ok(_) => panic!("duplicate provider key should return error"),
        }
    }

    #[test]
    fn duplicate_provider_model_keys_returns_error() {
        let providers = vec![provider_source(
            "alpha",
            provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
        )];
        let provider_models = vec![
            provider_model_source(ProviderIdx::new(0), "m1", info(4096, 512)),
            provider_model_source(ProviderIdx::new(0), "m1", info(4096, 512)),
        ];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::DuplicateKey {
                        table: LookupTableKind::ProviderModel,
                        key: "alpha/m1".to_string(),
                    }
                );
            }
            Ok(_) => panic!("duplicate provider-model key should return error"),
        }
    }

    #[test]
    fn same_model_key_across_providers_still_deduplicates_model_entries() {
        let providers = vec![
            provider_source(
                "alpha",
                provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
            ),
            provider_source(
                "beta",
                provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
            ),
        ];
        let provider_models = vec![
            provider_model_source(
                ProviderIdx::new(0),
                "m1",
                ModelInfo {
                    modalities: Modality::TEXT,
                    max_input: 4096,
                    max_output: 512,
                    temperature: Some(1.0),
                    top_p: Some(0.9),
                },
            ),
            provider_model_source(
                ProviderIdx::new(1),
                "m1",
                ModelInfo {
                    modalities: Modality::TEXT,
                    max_input: 4096,
                    max_output: 512,
                    temperature: Some(1.0),
                    top_p: Some(0.9),
                },
            ),
        ];

        let catalog =
            build_from_source(&providers, &provider_models).expect("source build should succeed");

        assert!(catalog.lookup("alpha", "m1").is_some());
        assert!(catalog.lookup("beta", "m1").is_some());
        assert_eq!(catalog.provider_model_count(), 2);
        assert_eq!(catalog.model_config_count(), 1);
    }

    #[test]
    fn provider_model_source_with_unknown_provider_returns_error() {
        let providers = vec![provider_source(
            "alpha",
            provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
        )];
        let provider_models = vec![provider_model_source(
            ProviderIdx::new(1),
            "m1",
            info(4096, 512),
        )];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::ProviderIdxOutOfRangeForModel {
                        provider_idx: ProviderIdx::new(1),
                        model_key: "m1".to_string(),
                    }
                );
            }
            Ok(_) => panic!("provider-model source with unknown provider should fail"),
        }
    }

    #[test]
    fn too_many_provider_env_vars_returns_error() {
        let providers = vec![provider_source(
            "alpha",
            provider(
                "https://alpha.example",
                &["A", "B", "C", "D", "E", "F", "G", "H"],
                ProviderType::Azure,
            ),
        )];
        let provider_models = vec![provider_model_source(
            ProviderIdx::new(0),
            "m1",
            info(4096, 512),
        )];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::TooManyProviderEnvVarsForOneProvider {
                        count: 8,
                        max: 7,
                    }
                );
            }
            Ok(_) => panic!("provider with too many env vars should fail"),
        }
    }

    #[test]
    fn max_output_tokens_out_of_range_returns_error() {
        let (providers, _) = test_sources();
        let max_output = super::MAX_OUTPUT_TOKENS;
        let provider_models = vec![provider_model_source(
            ProviderIdx::new(0),
            "m1",
            info(4096, max_output.saturating_add(1)),
        )];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::MaxOutputTokensOutOfRange {
                        max_output: max_output.saturating_add(1),
                        max: max_output,
                    }
                );
            }
            Ok(_) => panic!("max output over packed limit should fail"),
        }
    }

    #[test]
    fn max_input_tokens_out_of_range_returns_error() {
        let (providers, _) = test_sources();
        let max_input = super::MAX_INPUT_TOKENS;
        let provider_models = vec![provider_model_source(
            ProviderIdx::new(0),
            "m1",
            info(max_input.saturating_add(1), 512),
        )];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::MaxInputTokensOutOfRange {
                        max_input: max_input.saturating_add(1),
                        max: max_input,
                    }
                );
            }
            Ok(_) => panic!("max input over packed limit should fail"),
        }
    }

    #[test]
    fn too_many_total_env_vars_returns_error() {
        // 8192 providers * 1 env var = 8192, so the 8193rd provider would have
        // a start index of 8192, which exceeds MAX_ENV_START (8191).
        let mut providers = Vec::with_capacity(8193);
        for i in 0..8193usize {
            providers.push(provider_source(
                &format!("provider_{}", i),
                provider("https://example.com", &["VAR1"], ProviderType::Azure),
            ));
        }
        let mut provider_models = Vec::with_capacity(1);
        provider_models.push(provider_model_source(
            ProviderIdx::new(0),
            "m1",
            info(4096, 512),
        ));

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::TooManyEnvVarKeys {
                        count: 8_192,
                        max: 8_191,
                    }
                );
            }
            Ok(_) => panic!("too many total env vars should fail"),
        }
    }

    #[test]
    fn max_13bit_start_with_tail_entries_succeeds() {
        // The last provider's start index can be 8191 and still be valid when it
        // contributes keys at indices 8191 through 8197.
        let mut providers = Vec::with_capacity(1172);
        for i in 0..1170usize {
            providers.push(provider_source(
                &format!("provider_{}", i),
                provider(
                    "https://example.com",
                    &["VAR1", "VAR2", "VAR3", "VAR4", "VAR5", "VAR6", "VAR7"],
                    ProviderType::Azure,
                ),
            ));
        }
        providers.push(provider_source(
            "provider_1170",
            provider("https://example.com", &["VAR1"], ProviderType::Azure),
        ));
        providers.push(provider_source(
            "provider_1171",
            provider(
                "https://example.com",
                &["VAR1", "VAR2", "VAR3", "VAR4", "VAR5", "VAR6", "VAR7"],
                ProviderType::Azure,
            ),
        ));
        let provider_models = Vec::new();

        let catalog =
            build_from_source(&providers, &provider_models).expect("boundary case should pass");
        let provider = catalog
            .provider_from_index(ProviderIdx::new(1171))
            .expect("last provider should be addressable");

        assert_eq!(
            provider.env_vars(),
            &["VAR1", "VAR2", "VAR3", "VAR4", "VAR5", "VAR6", "VAR7"]
        );
    }
}
