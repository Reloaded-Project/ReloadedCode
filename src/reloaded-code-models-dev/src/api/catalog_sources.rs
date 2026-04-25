//! models.dev API -> `ModelCatalog` mapping.
//!
//! This module parses models.dev `api.json`, maps provider/model metadata into
//! transient core builder inputs, and immediately constructs a [`ModelCatalog`](reloaded_code_core::models::ModelCatalog).
//!
//! Mapping policy:
//! - missing limits default to `0`;
//! - model modalities are mapped from `modalities.input[]`/`modalities.output[]`
//!   into directional [`Modality`] flags;
//! - unknown npm package identifiers map to [`ProviderType::Unknown`];
//! - unknown modality labels are ignored; if nothing maps, modalities remain
//!   [`Modality::empty()`];
//! - model rows remain provider-scoped; shared configurations are deduplicated by
//!   core during catalog build.

use super::schema::{parse_api_json, ApiModelEntry, ApiModelLimit, ApiModelModalities};
use crate::cache::payload::{CachedModelRow, CachedProviderRow, CatalogCachePayload};
use crate::error::{CatalogError, CatalogResult};
use reloaded_code_core::models::{
    Modality, ModelCatalogBuildError, ModelInfo, ProviderIdx, ProviderType,
};

/// Parses models.dev API JSON bytes into a cache payload.
///
/// # Errors
/// - Returns [`CatalogError::Json`] when `json_bytes` cannot be parsed as valid
///   models.dev API JSON.
/// - Returns [`CatalogError::ModelCatalogBuild`] with [`ModelCatalogBuildError::TooManyProviders`]
///   when the number of providers exceeds `u16::MAX + 1` (65,536).
pub(crate) fn cache_payload_from_api_json_bytes(
    json_bytes: &[u8],
) -> CatalogResult<CatalogCachePayload> {
    let provider_entries = parse_api_json(json_bytes)?;

    let provider_count = provider_entries.len();
    if provider_count > (u16::MAX as usize) + 1 {
        return Err(CatalogError::ModelCatalogBuild(
            ModelCatalogBuildError::TooManyProviders {
                count: provider_count,
                max: (u16::MAX as usize) + 1,
            },
        ));
    }

    let mut providers = Vec::with_capacity(provider_count);
    let mut models = Vec::with_capacity(
        provider_entries
            .values()
            .map(|provider| provider.models.len())
            .sum(),
    );

    for (provider_key, provider) in provider_entries {
        let provider_idx = ProviderIdx::new(providers.len() as u16);
        let api_type = provider_type_from_models_dev_npm(provider.npm.as_deref());

        providers.push(CachedProviderRow {
            provider_key,
            api_url: provider.api.unwrap_or_default(),
            env_vars: provider.env,
            api_type,
        });

        for (model_key, model_entry) in provider.models {
            let model = model_info_from_entry(&model_entry);
            models.push(CachedModelRow {
                provider_idx,
                model_key,
                modalities_bits: model.modalities.bits(),
                max_input: model.max_input,
                max_output: model.max_output,
                temperature: model.temperature,
                top_p: model.top_p,
            });
        }
    }

    Ok(CatalogCachePayload { providers, models })
}

#[inline]
fn model_info_from_entry(model_entry: &ApiModelEntry) -> ModelInfo {
    let (max_input, max_output) = match model_entry.limit.as_ref() {
        Some(limit) => (model_max_input(limit), limit.output),
        None => (0, 0),
    };
    let modalities = model_modalities(model_entry.modalities.as_ref());

    ModelInfo {
        modalities,
        max_input,
        max_output,
        temperature: None,
        top_p: None,
    }
}

#[inline]
fn model_modalities(raw: Option<&ApiModelModalities>) -> Modality {
    let Some(raw) = raw else {
        return Modality::TEXT;
    };

    let mut modalities = Modality::empty();
    for label in &raw.input {
        modalities |= input_modality_flag(label.as_str());
    }
    for label in &raw.output {
        modalities |= output_modality_flag(label.as_str());
    }

    modalities
}

#[inline]
fn input_modality_flag(label: &str) -> Modality {
    match label {
        "text" => Modality::TEXT_INPUT,
        "image" => Modality::IMAGE_INPUT,
        "audio" => Modality::AUDIO_INPUT,
        "video" => Modality::VIDEO_INPUT,
        _ => Modality::empty(), // pdf not supported
    }
}

#[inline]
fn output_modality_flag(label: &str) -> Modality {
    match label {
        "text" => Modality::TEXT_OUTPUT,
        "image" => Modality::IMAGE_OUTPUT,
        "audio" => Modality::AUDIO_OUTPUT,
        "video" => Modality::VIDEO_OUTPUT,
        _ => Modality::empty(),
    }
}

#[inline]
fn model_max_input(limit: &ApiModelLimit) -> u32 {
    if limit.input == 0 {
        limit.context
    } else {
        limit.input
    }
}

#[inline]
fn provider_type_from_models_dev_npm(npm_package: Option<&str>) -> ProviderType {
    match npm_package {
        Some("@ai-sdk/openai") => ProviderType::OpenAiCompletions,
        Some("@ai-sdk/openai-compatible") => ProviderType::OpenAiCompletions,
        Some("@ai-sdk/openai-responses") => ProviderType::OpenAiResponses,
        Some("@ai-sdk/anthropic") => ProviderType::Anthropic,
        Some("@ai-sdk/google") => ProviderType::Google,
        Some("@ai-sdk/groq") => ProviderType::Groq,
        Some("@ai-sdk/mistral") => ProviderType::Mistral,
        Some("@ai-sdk/ollama") => ProviderType::Ollama,
        Some("@ai-sdk/amazon-bedrock") => ProviderType::Bedrock,
        Some("@ai-sdk/azure") => ProviderType::Azure,
        Some("@openrouter/ai-sdk-provider") => ProviderType::OpenRouter,
        Some("@ai-sdk/huggingface") => ProviderType::HuggingFace,
        Some("@ai-sdk/cohere") => ProviderType::Cohere,
        Some("@ai-sdk/chatgpt-oauth") => ProviderType::ChatGptOAuth,
        Some("@ai-sdk/claude-code-oauth") => ProviderType::ClaudeCodeOAuth,
        Some("@ai-sdk/antigravity") => ProviderType::Antigravity,
        Some(_) | None => ProviderType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{cache_payload_from_api_json_bytes, provider_type_from_models_dev_npm};
    use crate::cache::payload::catalog_from_cache_payload;
    use reloaded_code_core::models::{Modality, ModelCatalog, ProviderIdx, ProviderType};
    use rstest::rstest;

    fn catalog_from_api_json_bytes(json_bytes: &[u8]) -> crate::error::CatalogResult<ModelCatalog> {
        let payload = cache_payload_from_api_json_bytes(json_bytes)?;
        catalog_from_cache_payload(payload)
    }

    fn catalog(json: &[u8]) -> ModelCatalog {
        catalog_from_api_json_bytes(json).expect("API payload should map")
    }

    fn provider_snapshot(
        catalog: &ModelCatalog,
        provider_key: &str,
    ) -> (String, Vec<String>, ProviderType) {
        let provider = catalog
            .lookup_provider(provider_key)
            .expect("provider should exist");
        (
            provider.api_url.to_string(),
            provider
                .env_vars()
                .iter()
                .map(|env_var| (*env_var).to_string())
                .collect(),
            provider.api_type,
        )
    }

    fn model_snapshot(
        catalog: &ModelCatalog,
        provider_key: &str,
        model_key: &str,
    ) -> (Modality, u32, u32, Option<f32>, Option<f32>) {
        let model = catalog
            .lookup_provider_model(provider_key, model_key)
            .expect("provider model should exist");
        (
            model.modalities,
            model.max_input,
            model.max_output,
            model.temperature(),
            model.top_p(),
        )
    }

    #[test]
    fn cache_payload_maps_single_provider_with_models() {
        let api_json = br#"
        {
            "openai": {
                "npm": "@ai-sdk/openai",
                "api": "https://api.openai.com/v1",
                "env": ["OPENAI_API_KEY"],
                "models": {
                    "gpt-4": {
                        "modalities": { "input": ["text"], "output": ["text"] },
                        "limit": { "context": 8192, "output": 4096 }
                    }
                }
            }
        }
        "#;

        let payload = cache_payload_from_api_json_bytes(api_json).expect("payload should build");
        assert_eq!(payload.providers.len(), 1);
        assert_eq!(payload.models.len(), 1);

        assert_eq!(payload.providers[0].provider_key, "openai");
        assert_eq!(
            payload.providers[0].api_type,
            ProviderType::OpenAiCompletions
        );

        assert_eq!(payload.models[0].provider_idx, ProviderIdx::new(0));
        assert_eq!(payload.models[0].model_key, "gpt-4");
        assert_eq!(payload.models[0].modalities_bits, Modality::TEXT.bits());
        assert_eq!(payload.models[0].max_input, 8192);
        assert_eq!(payload.models[0].max_output, 4096);
    }

    #[test]
    fn catalog_source_mapping_maps_provider_rows() {
        let api_json = br#"
        {
            "alpha": {
                "npm": "@ai-sdk/openai-responses",
                "api": "https://alpha.example/v1",
                "env": ["ALPHA_KEY"],
                "models": {}
            }
        }
        "#;
        let catalog = catalog(api_json);

        assert_eq!(catalog.provider_count(), 1);
        let provider = catalog
            .lookup_provider("alpha")
            .expect("alpha provider should exist");
        assert_eq!(provider.api_url, "https://alpha.example/v1");
        assert_eq!(provider.env_vars(), ["ALPHA_KEY"]);
        assert_eq!(provider.api_type, ProviderType::OpenAiResponses);
    }

    #[test]
    fn catalog_source_mapping_defaults_missing_limits_to_zero() {
        let api_json = br#"
        {
            "alpha": {
                "npm": null,
                "api": null,
                "env": [],
                "models": {
                    "m1": {}
                }
            }
        }
        "#;
        let catalog = catalog(api_json);

        assert_eq!(catalog.provider_model_count(), 1);
        let model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 should exist");
        assert_eq!(model.modalities, Modality::TEXT);
        assert_eq!(model.max_input, 0);
        assert_eq!(model.max_output, 0);
    }

    #[test]
    fn catalog_source_mapping_uses_limit_input_when_present() {
        let api_json = br#"
        {
            "alpha": {
                "npm": null,
                "api": null,
                "env": [],
                "models": {
                    "m1": {
                        "limit": {
                            "context": 128000,
                            "input": 124000,
                            "output": 4096
                        }
                    }
                }
            }
        }
        "#;
        let catalog = catalog(api_json);

        let model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 should exist");
        assert_eq!(model.max_input, 124000);
        assert_eq!(model.max_output, 4096);
    }

    #[test]
    fn catalog_source_mapping_maps_directional_modalities() {
        let api_json = br#"
        {
            "alpha": {
                "npm": null,
                "api": null,
                "env": [],
                "models": {
                    "m1": {
                        "modalities": {
                            "input": ["text", "image", "pdf"],
                            "output": ["text", "audio"]
                        },
                        "limit": { "context": 4096, "output": 512 }
                    }
                }
            }
        }
        "#;

        let catalog = catalog(api_json);
        let model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 should exist");
        assert_eq!(
            model.modalities,
            Modality::TEXT_INPUT
                | Modality::TEXT_OUTPUT
                | Modality::IMAGE_INPUT
                | Modality::AUDIO_OUTPUT
        );
    }

    #[test]
    fn catalog_source_mapping_maps_pdf_input_to_empty() {
        let api_json = br#"
        {
            "alpha": {
                "npm": null,
                "api": null,
                "env": [],
                "models": {
                    "m1": {
                        "modalities": {
                            "input": ["pdf"],
                            "output": []
                        }
                    }
                }
            }
        }
        "#;

        let catalog = catalog(api_json);
        let model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 should exist");
        assert_eq!(model.modalities, Modality::empty());
    }

    #[test]
    fn catalog_source_mapping_falls_back_to_empty_for_unknown_modalities() {
        let api_json = br#"
        {
            "alpha": {
                "npm": null,
                "api": null,
                "env": [],
                "models": {
                    "m1": {
                        "modalities": {
                            "input": ["binary"],
                            "output": ["embedding"]
                        }
                    }
                }
            }
        }
        "#;

        let catalog = catalog(api_json);
        let model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 should exist");
        assert_eq!(model.modalities, Modality::empty());
    }

    #[test]
    fn catalog_source_mapping_keeps_duplicate_model_ids_per_provider() {
        let api_json = br#"
        {
            "alpha": {
                "npm": "@ai-sdk/openai",
                "api": null,
                "env": [],
                "models": {
                    "m1": {
                        "modalities": {
                            "input": ["image"],
                            "output": ["text"]
                        },
                        "limit": { "context": 4096, "output": 512 }
                    }
                }
            },
            "beta": {
                "npm": "@ai-sdk/anthropic",
                "api": null,
                "env": [],
                "models": {
                    "m1": {
                        "modalities": {
                            "input": ["audio"],
                            "output": ["video"]
                        },
                        "limit": { "context": 8192, "output": 256 }
                    }
                }
            }
        }
        "#;
        let catalog = catalog(api_json);

        assert_eq!(catalog.provider_model_count(), 2);

        let alpha_model = catalog
            .lookup_provider_model("alpha", "m1")
            .expect("alpha/m1 should exist");
        assert_eq!(alpha_model.max_input, 4096);
        assert_eq!(alpha_model.max_output, 512);
        assert_eq!(
            alpha_model.modalities,
            Modality::IMAGE_INPUT | Modality::TEXT_OUTPUT
        );

        let beta_model = catalog
            .lookup_provider_model("beta", "m1")
            .expect("beta/m1 should exist");
        assert_eq!(beta_model.max_input, 8192);
        assert_eq!(beta_model.max_output, 256);
        assert_eq!(
            beta_model.modalities,
            Modality::AUDIO_INPUT | Modality::VIDEO_OUTPUT
        );
    }

    #[test]
    fn catalog_source_mapping_keeps_same_data_for_different_input_key_order() {
        let api_json_a = br#"
        {
            "beta": {
                "npm": "@ai-sdk/anthropic",
                "api": null,
                "env": [],
                "models": {
                    "m2": { "limit": { "context": 2048, "output": 512 } }
                }
            },
            "alpha": {
                "npm": "@ai-sdk/openai",
                "api": null,
                "env": [],
                "models": {
                    "m1": { "limit": { "context": 1024, "output": 256 } }
                }
            }
        }
        "#;

        let api_json_b = br#"
        {
            "alpha": {
                "npm": "@ai-sdk/openai",
                "api": null,
                "env": [],
                "models": {
                    "m1": { "limit": { "context": 1024, "output": 256 } }
                }
            },
            "beta": {
                "npm": "@ai-sdk/anthropic",
                "api": null,
                "env": [],
                "models": {
                    "m2": { "limit": { "context": 2048, "output": 512 } }
                }
            }
        }
        "#;

        let catalog_a = catalog(api_json_a);
        let catalog_b = catalog(api_json_b);

        assert_eq!(catalog_a.provider_count(), catalog_b.provider_count());
        assert_eq!(
            catalog_a.provider_model_count(),
            catalog_b.provider_model_count()
        );
        assert_eq!(
            catalog_a.model_config_count(),
            catalog_b.model_config_count()
        );
        assert_eq!(
            provider_snapshot(&catalog_a, "alpha"),
            provider_snapshot(&catalog_b, "alpha")
        );
        assert_eq!(
            provider_snapshot(&catalog_a, "beta"),
            provider_snapshot(&catalog_b, "beta")
        );
        assert_eq!(
            model_snapshot(&catalog_a, "alpha", "m1"),
            model_snapshot(&catalog_b, "alpha", "m1")
        );
        assert_eq!(
            model_snapshot(&catalog_a, "beta", "m2"),
            model_snapshot(&catalog_b, "beta", "m2")
        );
    }

    /// Verifies that npm package names from AI SDK providers are correctly mapped
    /// to their corresponding ProviderType variants. Tests both known provider
    /// packages and the fallback case for unknown/missing packages.
    #[rstest]
    #[case::openai_package(Some("@ai-sdk/openai"), ProviderType::OpenAiCompletions)]
    #[case::openai_compatible_package(
        Some("@ai-sdk/openai-compatible"),
        ProviderType::OpenAiCompletions
    )]
    #[case::openai_responses_package(
        Some("@ai-sdk/openai-responses"),
        ProviderType::OpenAiResponses
    )]
    #[case::anthropic_package(Some("@ai-sdk/anthropic"), ProviderType::Anthropic)]
    #[case::google_package(Some("@ai-sdk/google"), ProviderType::Google)]
    #[case::groq_package(Some("@ai-sdk/groq"), ProviderType::Groq)]
    #[case::mistral_package(Some("@ai-sdk/mistral"), ProviderType::Mistral)]
    #[case::ollama_package(Some("@ai-sdk/ollama"), ProviderType::Ollama)]
    #[case::amazon_bedrock_package(Some("@ai-sdk/amazon-bedrock"), ProviderType::Bedrock)]
    #[case::azure_package(Some("@ai-sdk/azure"), ProviderType::Azure)]
    #[case::openrouter_provider_package(
        Some("@openrouter/ai-sdk-provider"),
        ProviderType::OpenRouter
    )]
    #[case::huggingface_package(Some("@ai-sdk/huggingface"), ProviderType::HuggingFace)]
    #[case::cohere_package(Some("@ai-sdk/cohere"), ProviderType::Cohere)]
    #[case::chatgpt_oauth_package(Some("@ai-sdk/chatgpt-oauth"), ProviderType::ChatGptOAuth)]
    #[case::claude_code_oauth_package(
        Some("@ai-sdk/claude-code-oauth"),
        ProviderType::ClaudeCodeOAuth
    )]
    #[case::antigravity_package(Some("@ai-sdk/antigravity"), ProviderType::Antigravity)]
    #[case::unknown_package(Some("@unknown/package"), ProviderType::Unknown)]
    #[case::missing_package_unknown(None, ProviderType::Unknown)]
    fn npm_package_maps_to_correct_provider_type(
        #[case] npm_package: Option<&str>,
        #[case] expected_provider_type: ProviderType,
    ) {
        assert_eq!(
            provider_type_from_models_dev_npm(npm_package),
            expected_provider_type
        );
    }
}
