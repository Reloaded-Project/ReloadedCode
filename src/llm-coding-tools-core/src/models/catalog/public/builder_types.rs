//! Types used when building a [`ModelCatalog`].

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
///
/// This type uses borrowed string slices to avoid unnecessary allocations,
/// as the builder will copy values into its internal storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderInfo<'a> {
    /// Base URL for this provider. Empty when unspecified.
    pub api_url: &'a str,
    /// Candidate environment variables used to resolve API keys.
    ///
    /// Order matters: callers may check these in order and use the first match.
    pub env_vars: &'a [&'a str],
    /// Type of API used by the provider.
    pub api_type: ProviderType,
}

/// Hash-table kind used in collision/build errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupTableKind {
    /// Provider-key lookup table.
    Provider,
    /// Model-key lookup table.
    Model,
}

impl core::fmt::Display for LookupTableKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Provider => f.write_str("provider"),
            Self::Model => f.write_str("model"),
        }
    }
}

/// Errors returned when building a [`crate::models::ModelCatalog`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
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
}
