//! Cache payload serialization for models.dev catalog data.
//!
//! The payload is stored as simple owned rows so it can be encoded compactly
//! with bitcode and rebuilt into a [`ModelCatalog`](llm_coding_tools_core::models::ModelCatalog)
//! without reparsing the original JSON.
//!
//! ## Compression Benchmark
//!
//! Using a 1.26 MB `api.json` snapshot (models.dev), converted to bitcode
//! then compressed with zstd at various levels:
//!
//! | Level          | Size      | % of JSON | Time    |
//! |----------------|-----------|-----------|---------|
//! | JSON           | 1260.7 KB | 100.00%   | -       |
//! | (raw bitcode)  | 105.7 KB  | 8.39%     | -       |
//! | 0              | 29.7 KB   | 2.36%     | 1.4ms   |
//! | 1              | 32.1 KB   | 2.55%     | 1.0ms   |
//! | 2              | 31.7 KB   | 2.51%     | 1.0ms   |
//! | 3              | 29.7 KB   | 2.36%     | 1.1ms   |
//! | 4              | 29.7 KB   | 2.36%     | 1.9ms   |
//! | 5              | 27.5 KB   | 2.18%     | 2.9ms   |
//! | 6              | 27.1 KB   | 2.15%     | 3.6ms   |
//! | 7              | 26.6 KB   | 2.11%     | 4.8ms   |
//! | 8              | 26.7 KB   | 2.12%     | 5.0ms   |
//! | 9              | 26.7 KB   | 2.12%     | 6.3ms   |
//! | 10             | 26.4 KB   | 2.09%     | 9.1ms   |
//! | 11             | 26.1 KB   | 2.07%     | 8.5ms   |
//! | 12             | 26.1 KB   | 2.07%     | 14.4ms  |
//! | 13             | 26.0 KB   | 2.06%     | 12.0ms  |
//! | 14             | 26.0 KB   | 2.06%     | 16.4ms  |
//! | 15             | 25.9 KB   | 2.06%     | 21.6ms  |
//! | 16             | 23.6 KB   | 1.87%     | 24.2ms  |
//! | 17             | 23.2 KB   | 1.84%     | 27.6ms  |
//! | 18             | 23.2 KB   | 1.84%     | 42.6ms  |
//! | 19             | 23.1 KB   | 1.83%     | 81.3ms  |
//! | 20             | 23.1 KB   | 1.83%     | 96.3ms  |
//! | 21             | 23.1 KB   | 1.83%     | 125.4ms |
//! | 22             | 23.1 KB   | 1.83%     | 207.5ms |
//!
//! Levels 1-3 offer the best speed/ratio tradeoff (~1ms, ~2.4% of JSON).
//! Levels 19-22 provide maximal compression but take 80-200ms.

use crate::error::{CatalogError, CatalogResult};
use llm_coding_tools_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
    ProviderSource, ProviderType,
};

/// Serializable cache representation of the models.dev catalog.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub(crate) struct CatalogCachePayload {
    /// Provider rows in catalog order.
    pub(crate) providers: Vec<CachedProviderRow>,
    /// Model rows that reference providers by index.
    pub(crate) models: Vec<CachedModelRow>,
}

/// Serializable provider row stored in the cache payload.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub(crate) struct CachedProviderRow {
    /// Stable provider lookup key.
    pub(crate) provider_key: String,
    /// Base API URL for requests to this provider.
    pub(crate) api_url: String,
    /// Environment variables that can supply credentials.
    pub(crate) env_vars: Vec<String>,
    /// Provider protocol or API shape.
    pub(crate) api_type: ProviderType,
}

/// Serializable model row stored in the cache payload.
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub(crate) struct CachedModelRow {
    /// Index into [`CatalogCachePayload::providers`].
    pub(crate) provider_idx: ProviderIdx,
    /// Stable model lookup key within the provider.
    pub(crate) model_key: String,
    /// Serialized [`Modality`] bitflags.
    pub(crate) modalities_bits: u8,
    /// Maximum supported input tokens.
    pub(crate) max_input: u32,
    /// Maximum supported output tokens.
    pub(crate) max_output: u32,
    /// Optional default temperature.
    pub(crate) temperature: Option<f32>,
    /// Optional default top-p value.
    pub(crate) top_p: Option<f32>,
}

/// Encodes a cache payload into bitcode bytes.
pub(crate) fn encode_cache_payload(payload: &CatalogCachePayload) -> Vec<u8> {
    bitcode::encode(payload)
}

/// Decodes bitcode bytes into an owned cache payload.
///
/// # Errors
///
/// Returns [`CatalogError::BitcodeDecode`] when the bytes are not a valid cache
/// payload encoding.
pub(crate) fn decode_cache_payload(bytes: &[u8]) -> CatalogResult<CatalogCachePayload> {
    bitcode::decode(bytes).map_err(|error| CatalogError::BitcodeDecode(error.to_string()))
}

/// Rebuilds a [`ModelCatalog`] from decoded cache rows.
///
/// # Errors
///
/// Returns [`CatalogError`] when any cached row data cannot be used to build a
/// valid catalog, such as when a model references an out-of-range provider.
pub(crate) fn catalog_from_cache_payload(
    payload: CatalogCachePayload,
) -> CatalogResult<ModelCatalog> {
    let CatalogCachePayload { providers, models } = payload;

    let mut provider_sources = Vec::with_capacity(providers.len());
    for row in providers {
        provider_sources.push(ProviderSource {
            provider_key: row.provider_key,
            provider: ProviderInfo {
                api_url: row.api_url,
                env_vars: row.env_vars,
                api_type: row.api_type,
            },
        });
    }

    let mut model_sources = Vec::with_capacity(models.len());
    for row in &models {
        model_sources.push(ProviderModelSource {
            provider_idx: row.provider_idx,
            model_key: row.model_key.as_str(),
            model: ModelInfo {
                modalities: Modality::from_bits_retain(row.modalities_bits),
                max_input: row.max_input,
                max_output: row.max_output,
                temperature: row.temperature,
                top_p: row.top_p,
            },
        });
    }

    Ok(ModelCatalog::build(&provider_sources, &model_sources)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_payload() -> CatalogCachePayload {
        CatalogCachePayload {
            providers: vec![CachedProviderRow {
                provider_key: "openai".to_string(),
                api_url: "https://api.openai.com/v1".to_string(),
                env_vars: vec!["OPENAI_API_KEY".to_string()],
                api_type: ProviderType::OpenAiCompletions,
            }],
            models: vec![CachedModelRow {
                provider_idx: ProviderIdx::new(0),
                model_key: "gpt-4".to_string(),
                modalities_bits: Modality::TEXT.bits(),
                max_input: 8192,
                max_output: 4096,
                temperature: Some(0.7),
                top_p: Some(0.9),
            }],
        }
    }

    #[test]
    fn payload_round_trip() {
        let original = sample_payload();
        let encoded = encode_cache_payload(&original);
        let decoded = decode_cache_payload(&encoded).expect("decode should succeed");
        assert_eq!(original, decoded);
    }

    #[test]
    fn catalog_from_payload_reconstructs_provider() {
        let payload = sample_payload();
        let catalog = catalog_from_cache_payload(payload).expect("catalog build should succeed");

        let provider = catalog
            .lookup_provider("openai")
            .expect("provider should exist");
        assert_eq!(provider.api_url, "https://api.openai.com/v1");
        assert_eq!(provider.api_type, ProviderType::OpenAiCompletions);
    }

    #[test]
    fn catalog_from_payload_reconstructs_model() {
        let payload = sample_payload();
        let catalog = catalog_from_cache_payload(payload).expect("catalog build should succeed");

        let model = catalog
            .lookup_provider_model("openai", "gpt-4")
            .expect("model should exist");
        assert_eq!(model.max_input, 8192);
        assert_eq!(model.max_output, 4096);
        assert_eq!(model.modalities, Modality::TEXT);
    }

    #[test]
    fn catalog_from_payload_rejects_out_of_range_provider_idx() {
        use llm_coding_tools_core::models::ModelCatalogBuildError;

        let payload = CatalogCachePayload {
            providers: vec![CachedProviderRow {
                provider_key: "test".to_string(),
                api_url: "".to_string(),
                env_vars: vec![],
                api_type: ProviderType::Unknown,
            }],
            models: vec![CachedModelRow {
                provider_idx: ProviderIdx::new(999),
                model_key: "model".to_string(),
                modalities_bits: Modality::TEXT.bits(),
                max_input: 0,
                max_output: 0,
                temperature: None,
                top_p: None,
            }],
        };

        let result = catalog_from_cache_payload(payload);
        assert!(matches!(
            result,
            Err(CatalogError::ModelCatalogBuild(
                ModelCatalogBuildError::ProviderIdxOutOfRangeForModel { .. }
            ))
        ));
    }

    #[test]
    fn all_known_provider_types_round_trip() {
        let types = [
            ProviderType::Unknown,
            ProviderType::OpenAiCompletions,
            ProviderType::OpenAiResponses,
            ProviderType::Anthropic,
            ProviderType::Google,
            ProviderType::Groq,
            ProviderType::Mistral,
            ProviderType::Ollama,
            ProviderType::Bedrock,
            ProviderType::Azure,
            ProviderType::OpenRouter,
            ProviderType::HuggingFace,
            ProviderType::Cohere,
            ProviderType::ChatGptOAuth,
            ProviderType::ClaudeCodeOAuth,
            ProviderType::Antigravity,
        ];

        for provider_type in types {
            let payload = CatalogCachePayload {
                providers: vec![CachedProviderRow {
                    provider_key: "test".to_string(),
                    api_url: "".to_string(),
                    env_vars: vec![],
                    api_type: provider_type,
                }],
                models: vec![],
            };

            let catalog = catalog_from_cache_payload(payload).expect("should succeed");
            let provider = catalog
                .lookup_provider("test")
                .expect("provider should exist");
            assert_eq!(
                provider.api_type, provider_type,
                "provider type should round-trip correctly"
            );
        }
    }
}
