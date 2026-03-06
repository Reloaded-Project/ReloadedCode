//! Types used when building a [`ModelCatalog`].
//!
//! [`ModelCatalog`]: crate::models::catalog::ModelCatalog

use super::Modality;
use crate::models::ProviderType;
use thiserror::Error;

/// Distilled per-model metadata used when inserting models during catalog construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelInfo {
    /// Content modalities this model can handle as input and/or output.
    pub modalities: Modality,
    /// Max input tokens.
    pub max_input: u32,
    /// Max output tokens.
    pub max_output: u32,
    /// Default sampling temperature, or `None` if unspecified.
    pub temperature: Option<f32>,
    /// Default sampling `top_p`, or `None` if unspecified.
    pub top_p: Option<f32>,
}

/// Distilled provider metadata used when inserting providers during catalog construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    /// Base URL for this provider. Empty when unspecified.
    pub api_url: String,
    /// Candidate environment variables used to resolve API keys.
    ///
    /// Order matters: callers may check these in order and use the first match.
    pub env_vars: Vec<String>,
    /// Type of API used by the provider.
    pub api_type: ProviderType,
}

/// Source that maps a provider key to provider metadata.
///
/// This wrapper keeps builder input self-documenting and avoids tuple-position
/// ambiguity at call sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSource {
    /// Provider identifier used by lookups (for example, `"openai"`).
    pub provider_key: String,
    /// Provider metadata associated with [`Self::provider_key`].
    pub provider: ProviderInfo,
}

impl ProviderSource {
    /// Creates a provider source.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - Provider identifier used during provider lookup.
    /// * `provider` - Provider metadata for this key.
    ///
    /// # Returns
    ///
    /// A new [`ProviderSource`].
    #[inline]
    pub fn new(provider_key: impl Into<String>, provider: ProviderInfo) -> Self {
        Self {
            provider_key: provider_key.into(),
            provider,
        }
    }
}

impl From<(String, ProviderInfo)> for ProviderSource {
    #[inline]
    fn from((provider_key, provider): (String, ProviderInfo)) -> Self {
        Self {
            provider_key,
            provider,
        }
    }
}

/// Source that maps a model under a specific provider to model metadata.
///
/// This wrapper keeps builder input self-documenting and avoids tuple-position
/// ambiguity at call sites.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderModelSource {
    /// Provider identifier used by lookups (for example, `"openai"`).
    pub provider_key: String,
    /// Model identifier used by lookups (for example, `"gpt-4"`).
    pub model_key: String,
    /// Model metadata associated with [`Self::model_key`].
    pub model: ModelInfo,
}

impl ProviderModelSource {
    /// Creates a provider model source.
    ///
    /// # Parameters
    ///
    /// * `provider_key` - Provider identifier used during provider lookup.
    /// * `model_key` - Model identifier used during model lookup for this provider.
    /// * `model` - Model metadata for this provider model.
    ///
    /// # Returns
    ///
    /// A new [`ProviderModelSource`].
    #[inline]
    pub fn new(
        provider_key: impl Into<String>,
        model_key: impl Into<String>,
        model: ModelInfo,
    ) -> Self {
        Self {
            provider_key: provider_key.into(),
            model_key: model_key.into(),
            model,
        }
    }
}

impl From<(String, String, ModelInfo)> for ProviderModelSource {
    #[inline]
    fn from((provider_key, model_key, model): (String, String, ModelInfo)) -> Self {
        Self {
            provider_key,
            model_key,
            model,
        }
    }
}

/// Hash-table kind used in collision/build errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupTableKind {
    /// Provider-key lookup table.
    Provider,
    /// Provider model lookup table.
    ProviderModel,
}

impl core::fmt::Display for LookupTableKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Provider => f.write_str("provider"),
            Self::ProviderModel => f.write_str("provider model"),
        }
    }
}

/// Errors returned when building a [`crate::models::ModelCatalog`].
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ModelCatalogBuildError {
    /// Provider count exceeds the `u16` provider-index address space.
    #[error("provider count {count} exceeds supported maximum {max}")]
    TooManyProviders {
        /// Number of providers supplied by the caller.
        count: usize,
        /// Maximum supported provider count.
        max: usize,
    },
    /// Unique model configuration count exceeds the `u16` index address space.
    #[error("model configuration count {count} exceeds supported maximum {max}")]
    TooManyModelConfigurations {
        /// Number of unique model configurations supplied by the caller.
        count: usize,
        /// Maximum supported unique model configuration count.
        max: usize,
    },
    /// One provider has too many env vars for the packed count field (max 3).
    #[error("provider env-var count {count} exceeds supported maximum {max}")]
    TooManyProviderEnvVarsForOneProvider {
        /// Number of env vars supplied for one provider.
        count: usize,
        /// Maximum supported env vars for one provider.
        max: usize,
    },
    /// A provider model source references a provider key that does not exist.
    #[error("provider model source references unknown provider_key={provider_key:?} for model_key={model_key:?}")]
    ProviderKeyNotFoundForModel {
        /// Provider key from the provider model source.
        provider_key: String,
        /// Model key from the provider model source.
        model_key: String,
    },
    /// Model output token limit exceeds packed-entry capacity.
    #[error("max_output {max_output} exceeds supported maximum {max}")]
    MaxOutputTokensOutOfRange {
        /// Max output token value from input model metadata.
        max_output: u32,
        /// Maximum representable output token value in packed entries.
        max: u32,
    },
    /// Model input token limit exceeds packed-entry capacity.
    #[error("max_input {max_input} exceeds supported maximum {max}")]
    MaxInputTokensOutOfRange {
        /// Max input token value from input model metadata.
        max_input: u32,
        /// Maximum representable input token value in packed entries.
        max: u32,
    },
    /// A hash collision was detected in one lookup table for the selected seed.
    #[error(
        "hash collision detected in {table} table for seed {seed}; reset builder and try next seed"
    )]
    HashCollision {
        /// Table where the collision was detected.
        table: LookupTableKind,
        /// Seed used by the builder when the collision was detected.
        seed: u8,
    },
    /// Collisions remained after all reseed attempts.
    #[error("hash collisions remain after {attempts} reseed attempts")]
    HashCollisionExhausted {
        /// Number of seeds attempted.
        attempts: u16,
    },
    /// Total env-var keys across all providers exceeds packed range capacity.
    #[error("total env-var keys {count} exceeds packed range capacity {max}")]
    TooManyEnvVarKeys {
        /// Total number of env vars across all providers.
        count: usize,
        /// Maximum representable env var keys count.
        max: usize,
    },
    /// String table capacity exceeded during construction.
    #[error("string table capacity exceeded: {0}")]
    StringTableCapacityExceeded(String),
    /// Invalid sampling value (negative or too large).
    #[error("invalid sampling value {field}={value}: must be >= 0.0 and <= 6.5535")]
    InvalidSamplingValue {
        /// Field name ("temperature" or "top_p").
        field: &'static str,
        /// Invalid value provided.
        value: f32,
    },
}
