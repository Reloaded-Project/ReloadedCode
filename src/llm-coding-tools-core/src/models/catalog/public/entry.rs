//! Lookup result types for provider and model queries.
//!
//! These are the flattened types returned by [`ModelCatalog`] lookup methods.
//! For builder input types, see [`ProviderInfo`](super::ProviderInfo),
//! [`ModelInfo`](super::ModelInfo), and [`ModelConfig`](super::ModelConfig).

use super::{ModelIdx, ProviderIdx};
use crate::models::catalog::internal::{Fixed4, Modality};
use crate::models::ProviderType;

/// Provider lookup result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider<'a> {
    /// Index into provider metadata tables.
    pub provider_idx: ProviderIdx,
    /// Provider base URL.
    pub api_url: &'a str,
    /// Candidate environment variables used to resolve API keys.
    pub env_vars: Vec<&'a str>,
    /// Type of API used by the provider.
    pub api_type: ProviderType,
}

/// Model lookup result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Index into model metadata/config sidecar tables.
    pub model_config_idx: ModelIdx,
    /// Content modalities this model can handle as input and/or output.
    pub modalities: Modality,
    /// Max input tokens.
    pub max_input: u32,
    /// Max output tokens.
    pub max_output: u32,
    temperature: Fixed4,
    top_p: Fixed4,
}

impl Model {
    /// Creates a new Model with the given parameters.
    pub(crate) const fn new(
        model_config_idx: ModelIdx,
        modalities: Modality,
        max_input: u32,
        max_output: u32,
        temperature: Fixed4,
        top_p: Fixed4,
    ) -> Self {
        Self {
            model_config_idx,
            modalities,
            max_input,
            max_output,
            temperature,
            top_p,
        }
    }

    /// Returns temperature as an `f32`, or `None` if not specified.
    #[inline]
    pub fn temperature(&self) -> Option<f32> {
        self.temperature.value()
    }

    /// Returns top_p as an `f32`, or `None` if not specified.
    #[inline]
    pub fn top_p(&self) -> Option<f32> {
        self.top_p.value()
    }

    /// Returns the raw Fixed4 temperature value (for internal use).
    #[inline]
    pub(crate) const fn temperature_fixed4(&self) -> Fixed4 {
        self.temperature
    }

    /// Returns the raw Fixed4 top_p value (for internal use).
    #[inline]
    pub(crate) const fn top_p_fixed4(&self) -> Fixed4 {
        self.top_p
    }
}

/// Joined provider + model lookup result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry<'a> {
    /// Index into provider metadata tables.
    pub provider_idx: ProviderIdx,
    /// Provider base URL.
    pub api_url: &'a str,
    /// Candidate environment variables used to resolve API keys.
    pub env_vars: Vec<&'a str>,
    /// Type of API used by the provider.
    pub api_type: ProviderType,
    /// Index into model metadata/config sidecar tables.
    pub model_config_idx: ModelIdx,
    /// Content modalities this model can handle as input and/or output.
    pub modalities: Modality,
    /// Max input tokens.
    pub max_input: u32,
    /// Max output tokens.
    pub max_output: u32,
    temperature: Fixed4,
    top_p: Fixed4,
}

impl<'a> CatalogEntry<'a> {
    /// Creates a new CatalogEntry with the given parameters.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        provider_idx: ProviderIdx,
        api_url: &'a str,
        env_vars: Vec<&'a str>,
        api_type: ProviderType,
        model_config_idx: ModelIdx,
        modalities: Modality,
        max_input: u32,
        max_output: u32,
        temperature: Fixed4,
        top_p: Fixed4,
    ) -> Self {
        Self {
            provider_idx,
            api_url,
            env_vars,
            api_type,
            model_config_idx,
            modalities,
            max_input,
            max_output,
            temperature,
            top_p,
        }
    }

    /// Returns temperature as an `f32`, or `None` if not specified.
    #[inline]
    pub fn temperature(&self) -> Option<f32> {
        self.temperature.value()
    }

    /// Returns top_p as an `f32`, or `None` if not specified.
    #[inline]
    pub fn top_p(&self) -> Option<f32> {
        self.top_p.value()
    }
}
