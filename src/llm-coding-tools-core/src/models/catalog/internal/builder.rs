use crate::models::catalog::internal::{
    hash_provider_key, hash_provider_model_key, hash_state_for_seed,
    provider_model_table_entry_hash, provider_table_entry_hash, ModelConfigEntry, PackedEnvRange,
    PackedModelEntry, PackedProviderModelTableEntry, PackedProviderTableEntry, MAX_ENV_RANGE_COUNT,
    MAX_ENV_START, MAX_INPUT_TOKENS, MAX_MODEL_CONFIG_COUNT, MAX_OUTPUT_TOKENS, MAX_PROVIDER_COUNT,
};
use crate::models::catalog::public::builder_types::{
    LookupTableKind, ModelCatalogBuildError, ProviderModelSourceRow, ProviderSourceRow,
};
use crate::models::catalog::public::ProviderIdx;
use crate::models::catalog::ModelCatalog;
use crate::models::ProviderType;
use ahash::AHashMap;
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

/// Builds a catalog from provider and provider-model source rows.
///
/// This is an internal construction path used by [`ModelCatalog::build`].
#[inline]
pub(crate) fn build_from_source(
    providers: &[ProviderSourceRow],
    provider_models: &[ProviderModelSourceRow],
) -> Result<ModelCatalog, ModelCatalogBuildError> {
    let provider_stats = analyze_provider_rows(providers)?;
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
    providers: &[ProviderSourceRow],
    provider_models: &[ProviderModelSourceRow],
) -> Result<(), ModelCatalogBuildError> {
    let mut env_start: u16 = 0;
    let mut provider_idx_by_key: AHashMap<&str, ProviderIdx> =
        AHashMap::with_capacity(providers.len());

    for provider_row in providers {
        let provider_info = &provider_row.provider;
        let env_count = provider_info.env_vars.len() as u8;
        let provider_idx = insert_provider_row(
            state,
            &provider_row.provider_key,
            env_start,
            env_count,
            provider_info.api_type,
        )?;
        provider_idx_by_key.insert(provider_row.provider_key.as_str(), provider_idx);

        // SAFETY: analyze_provider_rows bounds env_start and env_count (<= 3).
        env_start += u16::from(env_count);
    }

    for provider_model_row in provider_models {
        insert_provider_model_row(state, &provider_idx_by_key, provider_model_row)?;
    }

    Ok(())
}

#[inline]
fn insert_provider_row(
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
fn insert_provider_model_row(
    state: &mut BuildState,
    provider_idx_by_key: &AHashMap<&str, ProviderIdx>,
    provider_model_row: &ProviderModelSourceRow,
) -> Result<(), ModelCatalogBuildError> {
    if !provider_idx_by_key.contains_key(provider_model_row.provider_key.as_str()) {
        return Err(ModelCatalogBuildError::ProviderKeyNotFoundForModel {
            provider_key: provider_model_row.provider_key.clone(),
            model_key: provider_model_row.model_key.clone(),
        });
    }

    let info = provider_model_row.model;

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

    let key = hash_provider_model_key(
        &state.hash_state,
        &provider_model_row.provider_key,
        &provider_model_row.model_key,
    );
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
    providers: &[ProviderSourceRow],
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
fn analyze_provider_rows(
    providers: &[ProviderSourceRow],
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

    for provider_row in providers {
        // SAFETY: total_env_keys is the start index for this provider.
        // It must fit the 14-bit PackedEnvRange start field.
        if total_env_keys > max_env_start {
            return Err(ModelCatalogBuildError::TooManyEnvVarKeys {
                count: total_env_keys,
                max: max_env_start,
            });
        }

        let provider_info = &provider_row.provider;
        let env_count = provider_info.env_vars.len();
        // SAFETY: per-provider count must fit the 2-bit count field.
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
    providers: &[ProviderSourceRow],
    stats: ProviderSourceStats,
) -> Result<StringTable<u32, ProviderIdx>, ModelCatalogBuildError> {
    let mut builder = StringTableBuilder::<u32, ProviderIdx>::with_capacity_in(
        stats.provider_count,
        stats.total_api_url_bytes,
        Global,
    );

    for provider_row in providers {
        builder
            .try_push(&provider_row.provider.api_url)
            .map_err(|e| ModelCatalogBuildError::StringTableCapacityExceeded(e.to_string()))?;
    }

    Ok(builder.build())
}

#[inline]
fn build_provider_env_key_table(
    providers: &[ProviderSourceRow],
    stats: ProviderSourceStats,
) -> Result<StringTable<u32, ProviderIdx>, ModelCatalogBuildError> {
    let mut builder = StringTableBuilder::<u32, ProviderIdx>::with_capacity_in(
        stats.total_env_keys,
        stats.total_env_key_bytes,
        Global,
    );

    for provider_row in providers {
        for env_key in &provider_row.provider.env_vars {
            builder
                .try_push(env_key)
                .map_err(|e| ModelCatalogBuildError::StringTableCapacityExceeded(e.to_string()))?;
        }
    }

    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::{build_from_source, MAX_SEED};
    use crate::models::catalog::{
        Modality, ModelCatalogBuildError, ModelInfo, ProviderInfo, ProviderModelSourceRow,
        ProviderSourceRow,
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

    fn provider_row(provider_key: &str, provider: ProviderInfo) -> ProviderSourceRow {
        ProviderSourceRow::new(provider_key, provider)
    }

    fn provider_model_row(
        provider_key: &str,
        model_key: &str,
        model: ModelInfo,
    ) -> ProviderModelSourceRow {
        ProviderModelSourceRow::new(provider_key, model_key, model)
    }

    fn source_rows() -> (Vec<ProviderSourceRow>, Vec<ProviderModelSourceRow>) {
        (
            vec![provider_row(
                "alpha",
                provider(
                    "https://alpha.example",
                    &["ALPHA_KEY"],
                    ProviderType::OpenAiCompletions,
                ),
            )],
            vec![provider_model_row("alpha", "m1", info(4096, 512))],
        )
    }

    #[test]
    fn build_from_source_builds_catalog() {
        let (providers, provider_models) = source_rows();
        let catalog =
            build_from_source(&providers, &provider_models).expect("source build should succeed");

        assert_eq!(catalog.provider_count(), 1);
        assert_eq!(catalog.provider_model_count(), 1);
        assert!(catalog.lookup("alpha", "m1").is_some());
    }

    #[test]
    fn duplicate_provider_keys_exhaust_reseed_attempts() {
        let providers = vec![
            provider_row(
                "alpha",
                provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
            ),
            provider_row(
                "alpha",
                provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
            ),
        ];
        let provider_models = vec![provider_model_row("alpha", "m1", info(4096, 512))];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::HashCollisionExhausted {
                        attempts: MAX_SEED.into()
                    }
                );
            }
            Ok(_) => panic!("duplicate provider key should collide for all seeds"),
        }
    }

    #[test]
    fn duplicate_provider_model_keys_exhaust_reseed_attempts() {
        let providers = vec![provider_row(
            "alpha",
            provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
        )];
        let provider_models = vec![
            provider_model_row("alpha", "m1", info(4096, 512)),
            provider_model_row("alpha", "m1", info(4096, 512)),
        ];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::HashCollisionExhausted {
                        attempts: MAX_SEED.into()
                    }
                );
            }
            Ok(_) => panic!("duplicate provider-model key should collide for all seeds"),
        }
    }

    #[test]
    fn model_entries_are_deduplicated_by_info_and_config() {
        let providers = vec![
            provider_row(
                "alpha",
                provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
            ),
            provider_row(
                "beta",
                provider("https://beta.example", &["BETA_KEY"], ProviderType::Azure),
            ),
        ];
        let provider_models = vec![
            provider_model_row(
                "alpha",
                "m1",
                ModelInfo {
                    modalities: Modality::TEXT,
                    max_input: 4096,
                    max_output: 512,
                    temperature: Some(1.0),
                    top_p: Some(0.9),
                },
            ),
            provider_model_row(
                "beta",
                "m2",
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

        assert_eq!(catalog.provider_model_count(), 2);
        assert_eq!(catalog.model_config_count(), 1);
    }

    #[test]
    fn provider_model_row_with_unknown_provider_returns_error() {
        let providers = vec![provider_row(
            "alpha",
            provider("https://alpha.example", &["ALPHA_KEY"], ProviderType::Azure),
        )];
        let provider_models = vec![provider_model_row("beta", "m1", info(4096, 512))];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::ProviderKeyNotFoundForModel {
                        provider_key: "beta".to_string(),
                        model_key: "m1".to_string(),
                    }
                );
            }
            Ok(_) => panic!("provider-model row with unknown provider should fail"),
        }
    }

    #[test]
    fn too_many_provider_env_vars_returns_error() {
        let providers = vec![provider_row(
            "alpha",
            provider(
                "https://alpha.example",
                &["A", "B", "C", "D"],
                ProviderType::Azure,
            ),
        )];
        let provider_models = vec![provider_model_row("alpha", "m1", info(4096, 512))];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::TooManyProviderEnvVarsForOneProvider {
                        count: 4,
                        max: 3,
                    }
                );
            }
            Ok(_) => panic!("provider with too many env vars should fail"),
        }
    }

    #[test]
    fn max_output_tokens_out_of_range_returns_error() {
        let (providers, _) = source_rows();
        let max_output = super::MAX_OUTPUT_TOKENS;
        let provider_models = vec![provider_model_row(
            "alpha",
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
        let (providers, _) = source_rows();
        let max_input = super::MAX_INPUT_TOKENS;
        let provider_models = vec![provider_model_row(
            "alpha",
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
        // 5462 providers * 3 env vars = 16386, so the 5463rd provider would have
        // a start index of 16386, which exceeds MAX_ENV_START (16383).
        let mut providers = Vec::new();
        for i in 0..5463usize {
            providers.push(provider_row(
                &format!("provider_{}", i),
                provider(
                    "https://example.com",
                    &["VAR1", "VAR2", "VAR3"],
                    ProviderType::Azure,
                ),
            ));
        }
        let provider_models = vec![provider_model_row("provider_0", "m1", info(4096, 512))];

        match build_from_source(&providers, &provider_models) {
            Err(err) => {
                assert_eq!(
                    err,
                    ModelCatalogBuildError::TooManyEnvVarKeys {
                        count: 16_386,
                        max: 16_383,
                    }
                );
            }
            Ok(_) => panic!("too many total env vars should fail"),
        }
    }

    #[test]
    fn max_14bit_start_with_tail_entries_succeeds() {
        // The last provider's start index can be 16383 and still be valid when it
        // contributes 3 keys at indices 16383, 16384, and 16385.
        let mut providers = Vec::new();
        for i in 0..5462usize {
            providers.push(provider_row(
                &format!("provider_{}", i),
                provider(
                    "https://example.com",
                    &["VAR1", "VAR2", "VAR3"],
                    ProviderType::Azure,
                ),
            ));
        }
        let last_provider_key = format!("provider_{}", 5461usize);
        let provider_models = vec![provider_model_row(
            &last_provider_key,
            "m1",
            info(4096, 512),
        )];

        let catalog =
            build_from_source(&providers, &provider_models).expect("boundary case should pass");
        let entry = catalog
            .lookup(&last_provider_key, "m1")
            .expect("last provider should be addressable");

        assert_eq!(entry.env_vars.len(), 3);
    }
}
