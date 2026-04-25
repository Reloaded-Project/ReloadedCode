//! Minimal models.dev API schema used by this crate.
//!
//! We deserialize only fields needed for catalog-source mapping:
//! provider metadata (`npm`, `api`, `env`) and model token limits
//! (`limit.context`, `limit.input`, `limit.output`) plus directional modalities
//! (`modalities.input[]`, `modalities.output[]`).
//!
//! Representative payload shape from `https://models.dev/api.json`:
//!
//! ```json
//! {
//!   "openai": {
//!     "id": "openai",
//!     "npm": "@ai-sdk/openai",
//!     "api": null,
//!     "env": ["OPENAI_API_KEY"],
//!     "models": {
//!       "gpt-4o": {
//!         "id": "gpt-4o",
//!         "modalities": {
//!           "input": ["text", "image"],
//!           "output": ["text"]
//!         },
//!         "limit": {
//!           "context": 128000,
//!           "output": 16384
//!         }
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! Mapping into local structs:
//! - top-level provider map entry -> [`ApiProviderEntry`]
//! - `models.<model_id>` object -> [`ApiModelEntry`]
//! - `models.<model_id>.modalities` object -> [`ApiModelModalities`]
//! - `models.<model_id>.limit` object -> [`ApiModelLimit`]
//!
//! Unknown fields are intentionally ignored so we can drop large unused sections
//! early and keep parse memory bounded.

use crate::error::CatalogResult;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub(crate) struct ApiProviderEntry {
    #[serde(default)]
    pub(crate) npm: Option<String>,
    #[serde(default)]
    pub(crate) api: Option<String>,
    #[serde(default)]
    pub(crate) env: Vec<String>,
    #[serde(default)]
    pub(crate) models: HashMap<String, ApiModelEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiModelEntry {
    #[serde(default)]
    pub(crate) limit: Option<ApiModelLimit>,
    #[serde(default)]
    pub(crate) modalities: Option<ApiModelModalities>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiModelModalities {
    #[serde(default)]
    pub(crate) input: Vec<String>,
    #[serde(default)]
    pub(crate) output: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiModelLimit {
    #[serde(default)]
    pub(crate) context: u32,
    #[serde(default)]
    pub(crate) input: u32,
    #[serde(default)]
    pub(crate) output: u32,
}

/// Parses upstream `api.json` bytes into a provider map.
///
/// Input must match the current models.dev shape: a flat top-level object where
/// each key is a provider id and each value is a provider entry.
///
/// # Errors
/// - Returns [`CatalogError::Json`] when `json_bytes` is not valid JSON or does not
///   match the expected models.dev API schema structure.
///
/// [`CatalogError::Json`]: crate::error::CatalogError::Json
#[inline]
pub(crate) fn parse_api_json(
    json_bytes: &[u8],
) -> CatalogResult<HashMap<String, ApiProviderEntry>> {
    Ok(serde_json::from_slice(json_bytes)?)
}

#[cfg(test)]
mod tests {
    use super::parse_api_json;

    #[test]
    fn parse_api_json_supports_flat_provider_map() {
        let api_json = br#"{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_KEY"],"models":{"m1":{"modalities":{"input":["text","image"],"output":["text"]},"limit":{"context":4096,"output":512}}}}}"#;
        let providers = parse_api_json(api_json).expect("API payload should parse");
        let provider = providers.get("alpha").expect("provider should exist");

        assert_eq!(provider.npm.as_deref(), Some("@ai-sdk/openai"));
        assert_eq!(provider.env.as_slice(), ["ALPHA_KEY"]);

        let model = provider.models.get("m1").expect("model should exist");
        let modalities = model.modalities.as_ref().expect("modalities should exist");
        let limit = model.limit.as_ref().expect("limit should exist");
        assert_eq!(modalities.input.as_slice(), ["text", "image"]);
        assert_eq!(modalities.output.as_slice(), ["text"]);
        assert_eq!(limit.context, 4096);
        assert_eq!(limit.output, 512);
    }

    #[test]
    fn parse_api_json_ignores_unknown_fields() {
        let api_json = br#"
        {
            "alpha": {
                "id": "alpha",
                "name": "Alpha",
                "npm": "@ai-sdk/openai",
                "api": "https://alpha.example/v1",
                "env": ["ALPHA_KEY"],
                "models": {
                    "m1": {
                        "description": "ignored",
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

        let providers = parse_api_json(api_json).expect("API payload should parse");
        let provider = providers.get("alpha").expect("provider should exist");
        let model = provider.models.get("m1").expect("model should exist");
        let limit = model.limit.as_ref().expect("limit should exist");

        assert_eq!(limit.context, 128000);
        assert_eq!(limit.input, 124000);
        assert_eq!(limit.output, 4096);
    }
}
