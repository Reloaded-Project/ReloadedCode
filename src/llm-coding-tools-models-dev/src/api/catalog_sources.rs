//! models.dev API -> `ModelCatalog` mapping.
//!
//! This module parses models.dev `api.json`, maps provider/model metadata into
//! transient core builder inputs, and immediately constructs a [`ModelCatalog`].
//!
//! Mapping policy:
//! - missing limits default to `0`;
//! - model modalities are mapped from `modalities.input[]`/`modalities.output[]`
//!   into directional [`Modality`] flags;
//! - unknown npm package identifiers map to [`ProviderType::Unknown`];
//! - unknown modality labels are ignored; if nothing maps, modalities default to
//!   [`Modality::TEXT`];
//! - model rows remain provider-scoped; shared configurations are deduplicated by
//!   core during catalog build.

use super::schema::{parse_api_json, ApiModelEntry, ApiModelLimit, ApiModelModalities};
use crate::error::CatalogResult;
use llm_coding_tools_core::models::{
    Modality, ModelCatalog, ModelInfo, ProviderInfo, ProviderModelSource, ProviderSource,
    ProviderType,
};

/// Parses models.dev `api.json` bytes and builds a [`ModelCatalog`].
pub(crate) fn catalog_from_api_json_bytes(json_bytes: &[u8]) -> CatalogResult<ModelCatalog> {
    let provider_entries = parse_api_json(json_bytes)?;
    let mut provider_model_count = 0usize;
    for provider in provider_entries.values() {
        provider_model_count = provider_model_count.saturating_add(provider.models.len());
    }

    let mut provider_rows = Vec::with_capacity(provider_entries.len());
    let mut model_rows = Vec::with_capacity(provider_model_count);

    for (provider_key, provider) in &provider_entries {
        debug_assert!(provider.id.is_empty() || provider.id == *provider_key);

        let api_type = provider_type_from_models_dev_npm(provider.npm.as_deref());
        for (model_key, model_entry) in &provider.models {
            model_rows.push(ProviderModelSource::new(
                provider_key.as_str(),
                model_key.as_str(),
                model_info_from_entry(model_entry),
            ));
        }

        provider_rows.push(ProviderSource::new(
            provider_key.as_str(),
            ProviderInfo {
                api_url: provider.api.clone().unwrap_or_default(),
                env_vars: provider.env.clone(),
                api_type,
            },
        ));
    }

    Ok(ModelCatalog::build(&provider_rows, &model_rows)?)
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

    if modalities.is_empty() {
        Modality::TEXT
    } else {
        modalities
    }
}

#[inline]
fn input_modality_flag(label: &str) -> Modality {
    match label {
        "text" => Modality::TEXT_INPUT,
        "image" => Modality::IMAGE_INPUT,
        "audio" => Modality::AUDIO_INPUT,
        "video" => Modality::VIDEO_INPUT,
        // `pdf` appears in models.dev input modalities. Core has no PDF bit yet,
        // so map it to text-input capability as closest supported fallback.
        "pdf" => Modality::TEXT_INPUT,
        _ => Modality::empty(),
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
    use super::{catalog_from_api_json_bytes, provider_type_from_models_dev_npm};
    use llm_coding_tools_core::models::{Modality, ModelCatalog, ProviderType};

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
    fn catalog_source_mapping_maps_provider_rows() {
        let api_json = br#"
        {
            "alpha": {
                "id": "alpha",
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
                "id": "alpha",
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
                "id": "alpha",
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
                "id": "alpha",
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
    fn catalog_source_mapping_maps_pdf_input_to_text_input() {
        let api_json = br#"
        {
            "alpha": {
                "id": "alpha",
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
        assert_eq!(model.modalities, Modality::TEXT_INPUT);
    }

    #[test]
    fn catalog_source_mapping_falls_back_to_text_for_unknown_modalities() {
        let api_json = br#"
        {
            "alpha": {
                "id": "alpha",
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
        assert_eq!(model.modalities, Modality::TEXT);
    }

    #[test]
    fn catalog_source_mapping_keeps_duplicate_model_ids_per_provider() {
        let api_json = br#"
        {
            "alpha": {
                "id": "alpha",
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
                "id": "beta",
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
                "id": "beta",
                "npm": "@ai-sdk/anthropic",
                "api": null,
                "env": [],
                "models": {
                    "m2": { "limit": { "context": 2048, "output": 512 } }
                }
            },
            "alpha": {
                "id": "alpha",
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
                "id": "alpha",
                "npm": "@ai-sdk/openai",
                "api": null,
                "env": [],
                "models": {
                    "m1": { "limit": { "context": 1024, "output": 256 } }
                }
            },
            "beta": {
                "id": "beta",
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

    #[test]
    fn provider_type_mapping_handles_known_and_unknown_packages() {
        assert_eq!(
            provider_type_from_models_dev_npm(Some("@ai-sdk/openai")),
            ProviderType::OpenAiCompletions
        );
        assert_eq!(
            provider_type_from_models_dev_npm(Some("@ai-sdk/google")),
            ProviderType::Google
        );
        assert_eq!(
            provider_type_from_models_dev_npm(Some("@ai-sdk/openai-compatible")),
            ProviderType::Unknown
        );
        assert_eq!(
            provider_type_from_models_dev_npm(None),
            ProviderType::Unknown
        );
    }
}
