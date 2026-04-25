//! Resolve effective runtime model selections for SerdesAI agents.
//!
//! Converts an agent's model selection into a validated `provider:model` format.
//! The model is resolved against a provided catalog to ensure it exists and is
//! properly configured.

use reloaded_code_agents::{
    AgentConfig, AgentDefaults, ModelResolutionError, ResolvedModel, resolve_model_with_catalog,
};
use reloaded_code_core::models::ModelCatalog;

/// Resolves the effective runtime model for an agent and validates it against the catalog.
///
/// Returns an error if the model is missing, malformed, or not found in the catalog.
#[inline]
pub(super) fn resolve_model(
    catalog: &ModelCatalog,
    defaults: &AgentDefaults,
    agent: &AgentConfig,
) -> Result<ResolvedModel, ModelResolutionError> {
    resolve_model_with_catalog(catalog, defaults, agent)
}

#[cfg(test)]
mod tests {
    use super::resolve_model;
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use reloaded_code_agents::{
        AgentConfig, AgentDefaults, AgentMode, AgentToolSettings, ModelResolutionError,
    };
    use reloaded_code_core::models::{
        Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
        ProviderSource, ProviderType,
    };

    /// Creates a minimal agent config with an optional model override.
    fn config_with_model(name: &str, model: Option<&str>) -> AgentConfig {
        AgentConfig {
            name: name.into(),
            mode: AgentMode::All,
            description: Default::default(),
            model: model.map(Into::into),
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: AHashMap::new(),
            tool_settings: AgentToolSettings::default(),
            prompt: Default::default(),
        }
    }

    /// Creates a provider info struct for the catalog.
    fn provider(api_url: &str, env_vars: &[&str], api_type: ProviderType) -> ProviderInfo {
        ProviderInfo {
            api_url: api_url.to_string(),
            env_vars: env_vars.iter().map(|v| (*v).to_string()).collect(),
            api_type,
        }
    }

    /// Creates model info with standard text modalities.
    fn model_info(max_input: u32, max_output: u32) -> ModelInfo {
        ModelInfo {
            modalities: Modality::TEXT,
            max_input,
            max_output,
            temperature: Some(1.0),
            top_p: Some(0.95),
        }
    }

    /// Builds a catalog from provider and model definitions.
    fn build_catalog(
        providers: Vec<(&str, ProviderInfo)>,
        provider_models: Vec<(&str, &str, ModelInfo)>,
    ) -> ModelCatalog {
        let provider_sources: Vec<ProviderSource> = providers
            .into_iter()
            .map(|(key, info)| ProviderSource::new(key, info))
            .collect();
        let provider_model_sources: Vec<ProviderModelSource<'_>> = provider_models
            .into_iter()
            .map(|(provider_key, model_key, info)| {
                let provider_idx = ProviderIdx::new(
                    provider_sources
                        .iter()
                        .position(|p| p.provider_key == provider_key)
                        .expect("provider key should exist") as u16,
                );
                ProviderModelSource::new(provider_idx, model_key, info)
            })
            .collect();
        ModelCatalog::build(&provider_sources, &provider_model_sources)
            .expect("catalog fixture should build")
    }

    #[test]
    fn resolve_model_accepts_valid_provider_model_pairs() {
        // Catalog with one provider and model
        let catalog = build_catalog(
            vec![(
                "openrouter",
                provider(
                    "https://openrouter.ai/api/v1",
                    &["OPENROUTER_API_KEY"],
                    ProviderType::OpenRouter,
                ),
            )],
            vec![(
                "openrouter",
                "openai/gpt-4.1-mini",
                model_info(128_000, 16_384),
            )],
        );
        // Agent uses default model, no override
        let defaults = AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini");
        let agent = config_with_model("planner", None);

        // Should resolve to provider and model components
        let resolved = resolve_model(&catalog, &defaults, &agent).expect("model should resolve");
        assert_eq!(resolved.provider(), "openrouter");
        assert_eq!(resolved.model(), "openai/gpt-4.1-mini");
    }

    #[test]
    fn resolve_model_preserves_model_resolution_errors() {
        // Catalog with openai provider and gpt-4o model
        let catalog = build_catalog(
            vec![(
                "openai",
                provider(
                    "https://api.openai.com/v1",
                    &["OPENAI_API_KEY"],
                    ProviderType::OpenAiResponses,
                ),
            )],
            vec![("openai", "gpt-4o", model_info(128_000, 16_384))],
        );

        // Agent requests a model that doesn't exist in catalog
        let defaults = AgentDefaults::default();
        let agent = config_with_model("planner", Some("openai/gpt-4.1-mini"));
        // Should return unknown model error with details
        let err = resolve_model(&catalog, &defaults, &agent).expect_err("should fail");

        match err {
            ModelResolutionError::UnknownModel {
                location,
                provider,
                model,
                ..
            } => {
                assert_eq!(location, "agent override");
                assert_eq!(&*provider, "openai");
                assert_eq!(&*model, "gpt-4.1-mini");
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
