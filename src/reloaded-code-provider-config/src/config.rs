//! Serde deserialization shapes for YAML provider configuration.
//!
//! The top-level YAML document is an [`IndexMap`] of provider keys to
//! [`ProviderConfig`] values. Each provider maps its models to
//! [`ModelConfig`] values.
//!
//! ```yaml
//! # ── IndexMap<String, ProviderConfig> ──────────────────────────
//! my-provider:                       # provider key (map key)
//!   # ── ProviderConfig ────────────
//!   api_url: https://api.example.com/v1
//!   api_type: openai-compatible      # optional; defaults to "openai-compatible"
//!   env:                             # optional; checked in order by CredentialResolver
//!     - MY_PROVIDER_API_KEY
//!   models:
//!     # ── ModelConfig ────────────
//!     gpt-4o:                        # model key (map key)
//!       max_input: 128000            # required
//!       max_output: 8192             # required
//!       modalities: [text, image]    # optional; defaults to ["text"]
//!       default_temperature: 0.7     # optional; maps to ModelInfo::temperature
//!       default_top_p: 0.95          # optional; maps to ModelInfo::top_p
//! ```
//!
//! After deserialization, [`ProviderConfig`] and [`ModelConfig`] are converted
//! to [`reloaded_code_core::models::ProviderInfo`] and
//! [`reloaded_code_core::models::ModelInfo`] respectively, then discarded.

use indexmap::IndexMap;
use serde::Deserialize;

/// Per-provider configuration parsed from YAML.
///
/// This is a serde deserialization shape only. After loading, it is converted
/// to [`reloaded_code_core::models::ProviderInfo`] and discarded.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    /// Base URL for the provider API. Required for openai-compatible providers.
    pub api_url: Option<String>,
    /// API type string, mapped via [`crate::api_type::api_type_from_str`].
    /// Defaults to `"openai-compatible"` when omitted.
    #[allow(rustdoc::private_intra_doc_links)]
    pub api_type: Option<String>,
    /// Environment variable names checked by `CredentialResolver`, in order.
    pub env: Option<Vec<String>>,
    /// Models offered by this provider. Key is the model identifier.
    pub models: Option<IndexMap<String, ModelConfig>>,
}

/// Per-model configuration parsed from YAML.
///
/// This is a serde deserialization shape only. After loading, it is converted
/// to [`reloaded_code_core::models::ModelInfo`] and discarded.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    /// Maximum input token count. Required.
    pub max_input: u32,
    /// Maximum output token count. Required.
    pub max_output: u32,
    /// Content modalities this model supports. Defaults to `["text"]`.
    #[serde(default = "default_modalities")]
    pub modalities: Vec<String>,
    /// Default sampling temperature. Maps to `ModelInfo::temperature`.
    pub default_temperature: Option<f32>,
    /// Default nucleus sampling value. Maps to `ModelInfo::top_p`.
    pub default_top_p: Option<f32>,
}

fn default_modalities() -> Vec<String> {
    vec!["text".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_provider_config() {
        let yaml = indoc::indoc! {"
            api_url: https://api.example.com/v1
            api_type: openai-compatible
            env:
              - EXAMPLE_API_KEY
            models:
              my-model:
                max_input: 128000
                max_output: 8192
                modalities:
                  - text
                  - image
                default_temperature: 0.7
                default_top_p: 0.95
        "};
        let config: ProviderConfig = serde_yaml::from_str(yaml).expect("should parse");
        assert_eq!(
            config.api_url.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(config.api_type.as_deref(), Some("openai-compatible"));
        assert_eq!(config.env.as_ref().unwrap().len(), 1);
        let models = config.models.as_ref().expect("should have models");
        assert_eq!(models.len(), 1);
        let model = &models["my-model"];
        assert_eq!(model.max_input, 128000);
        assert_eq!(model.max_output, 8192);
        assert_eq!(model.modalities, vec!["text", "image"]);
        assert_eq!(model.default_temperature, Some(0.7));
        assert_eq!(model.default_top_p, Some(0.95));
    }

    #[test]
    fn deserialize_minimal_provider_config() {
        let yaml = indoc::indoc! {"
            api_url: https://api.example.com/v1
            models:
              tiny:
                max_input: 4096
                max_output: 2048
        "};
        let config: ProviderConfig = serde_yaml::from_str(yaml).expect("should parse");
        assert!(config.api_type.is_none());
        assert!(config.env.is_none());
        let model = &config.models.as_ref().unwrap()["tiny"];
        assert_eq!(model.modalities, vec!["text"]); // default
        assert!(model.default_temperature.is_none());
        assert!(model.default_top_p.is_none());
    }

    #[test]
    fn deserialize_multiple_providers() {
        let yaml = indoc::indoc! {"
            provider-a:
              api_url: https://a.example.com/v1
              models:
                m1: { max_input: 8192, max_output: 4096 }
            provider-b:
              api_url: https://b.example.com/v1
              api_type: anthropic
              env: [ B_API_KEY ]
              models:
                m2: { max_input: 200000, max_output: 8192 }
        "};
        let map: IndexMap<String, ProviderConfig> =
            serde_yaml::from_str(yaml).expect("should parse");
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("provider-a"));
        assert!(map.contains_key("provider-b"));
    }
}
