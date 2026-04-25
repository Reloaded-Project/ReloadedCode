//! Picks which model an agent uses.
//!
//! An agent can specify its own model, or fall back to the runtime default.
//! This module validates that the chosen model exists in your catalog.
//!
//! # Public API
//!
//! - [`resolve_model_with_catalog()`] - Picks which model an agent will use
//! - [`ResolvedModel`] - A model identifier that's been validated
//! - [`ModelResolutionError`] - Errors when model selection fails
//!
//! # Precedence
//!
//! 1. If the agent's markdown file specifies a model, use that
//! 2. Otherwise, use the default from [`AgentDefaults`]
//! 3. If neither is set, return [`ModelResolutionError::MissingEffectiveModel`]
//!
//! # Identifier Format
//!
//! Models use `provider/model-id` format, like `openai/gpt-5.4` or
//! `ollama-cloud/minimax-m2.7`. Invalid formats (missing `/`
//! or empty segments) produce [`ModelResolutionError::MalformedModelIdentifier`].
//!
//! # Validation
//!
//! The resolved model is validated against a [`ModelCatalog`]:
//! - Unknown provider → [`ModelResolutionError::UnknownProvider`]
//! - Unknown model for that provider → [`ModelResolutionError::UnknownModel`]
//!
//! [`AgentDefaults`]: super::state::AgentDefaults
//! [`ModelCatalog`]: reloaded_code_core::models::ModelCatalog

use crate::AgentConfig;
use reloaded_code_core::models::ModelCatalog;

/// A model identifier that's been validated against your catalog.
///
/// Use [`provider()`][`Self::provider()`] and [`model()`][`Self::model()`] to get the
/// parts, or [`slash_spec()`][`Self::slash_spec()`] for the combined `provider/model-id` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModel {
    provider: Box<str>,
    model: Box<str>,
}

impl ResolvedModel {
    /// Returns the provider (e.g., `openai`).
    #[inline]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Returns the model name within the provider.
    #[inline]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns `provider/model-id` format.
    #[inline]
    pub fn slash_spec(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

/// Errors when picking or validating a model.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelResolutionError {
    /// Model string is malformed (missing `/` or empty parts).
    MalformedModelIdentifier {
        /// Agent name for error context.
        agent: Box<str>,
        /// Where the bad model string came from.
        location: &'static str,
        /// The malformed model string.
        model: Box<str>,
    },
    /// Neither the agent nor the runtime default specifies a model.
    MissingEffectiveModel {
        /// Agent name for error context.
        agent: Box<str>,
    },
    /// The provider isn't in the catalog.
    UnknownProvider {
        /// Agent name for error context.
        agent: Box<str>,
        /// Where the provider came from.
        location: &'static str,
        /// The unknown provider.
        provider: Box<str>,
    },
    /// The model isn't in the catalog for this provider.
    UnknownModel {
        /// Agent name for error context.
        agent: Box<str>,
        /// Where the model came from.
        location: &'static str,
        /// Provider.
        provider: Box<str>,
        /// Model name within the provider.
        model: Box<str>,
    },
}

impl core::fmt::Display for ModelResolutionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MalformedModelIdentifier {
                agent,
                location,
                model,
            } => write!(
                f,
                "agent `{agent}` has malformed {location} model `{model}`; expected `provider/model-id`",
            ),
            Self::MissingEffectiveModel { agent } => write!(
                f,
                "agent `{agent}` does not define a model override and runtime defaults do not define one either",
            ),
            Self::UnknownProvider {
                agent: _,
                location,
                provider,
            } => {
                write!(
                    f,
                    "effective provider `{provider}` from {location} is not in catalog"
                )
            }
            Self::UnknownModel {
                agent: _,
                location,
                provider,
                model,
            } => write!(
                f,
                "effective model `{provider}/{model}` from {location} is not in catalog",
            ),
        }
    }
}

impl std::error::Error for ModelResolutionError {}

/// Picks which model an agent will use.
///
/// Checks the agent's model first, then the runtime default.
/// Validates the result against the catalog.
///
/// # Arguments
///
/// * `catalog` - Your model catalog for validation
/// * `defaults` - Default settings (used if agent doesn't specify a model)
/// * `agent` - The agent configuration
///
/// # Returns
///
/// A [`ResolvedModel`] on success, or a [`ModelResolutionError`] if something's wrong.
pub fn resolve_model_with_catalog(
    catalog: &ModelCatalog,
    defaults: &super::state::AgentDefaults,
    agent: &AgentConfig,
) -> Result<ResolvedModel, ModelResolutionError> {
    let (provider, model, location) = get_provider_model(defaults, agent)?;

    if catalog.lookup_provider(provider).is_none() {
        return Err(ModelResolutionError::UnknownProvider {
            agent: agent.name.clone(),
            location,
            provider: provider.into(),
        });
    }

    if catalog.lookup_provider_model(provider, model).is_none() {
        return Err(ModelResolutionError::UnknownModel {
            agent: agent.name.clone(),
            location,
            provider: provider.into(),
            model: model.into(),
        });
    }

    Ok(ResolvedModel {
        provider: provider.into(),
        model: model.into(),
    })
}

/// Resolves the effective provider and model for an agent.
///
/// Checks sources in precedence order: agent-level override first, then runtime defaults.
/// Returns the parsed `(provider, model)` tuple along with a string identifying which
/// source was used (`"agent override"` or `"runtime default"`).
///
/// # Examples
///
/// ```text
/// Agent has its own model set: "openai/gpt-5.4"
/// Defaults also has a model set: "ollama-cloud/minimax-m2.7"
/// Result: ("openai", "gpt-5.4", "agent override")
/// The agent's model wins because it was set directly.
/// ```
///
/// ```text
/// Agent has no model set.
/// Defaults has model set: "anthropic/claude-3-5-sonnet"
/// Result: ("anthropic", "claude-3-5-sonnet", "runtime default")
/// Falls back to the default since agent didn't specify one.
/// ```
///
/// # Errors
///
/// Returns [`ModelResolutionError::MalformedModelIdentifier`] if the model string
/// from either source cannot be parsed (missing `/` separator).
/// Returns [`ModelResolutionError::MissingEffectiveModel`] if neither source provides a model.
fn get_provider_model<'a>(
    defaults: &'a super::state::AgentDefaults,
    agent: &'a AgentConfig,
) -> Result<(&'a str, &'a str, &'static str), ModelResolutionError> {
    if let Some(raw) = agent.model.as_deref() {
        let (provider, model) = crate::parse_model_parts(raw).ok_or_else(|| {
            ModelResolutionError::MalformedModelIdentifier {
                agent: agent.name.clone(),
                location: "agent override",
                model: raw.into(),
            }
        })?;
        return Ok((provider, model, "agent override"));
    }

    if let Some(raw) = defaults.model.as_deref() {
        let (provider, model) = crate::parse_model_parts(raw).ok_or_else(|| {
            ModelResolutionError::MalformedModelIdentifier {
                agent: agent.name.clone(),
                location: "runtime default",
                model: raw.into(),
            }
        })?;
        return Ok((provider, model, "runtime default"));
    }

    Err(ModelResolutionError::MissingEffectiveModel {
        agent: agent.name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_model_with_catalog, ModelResolutionError};
    use crate::runtime::AgentDefaults;
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use reloaded_code_core::models::{
        Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
        ProviderSource, ProviderType,
    };

    fn config_with_model(name: &str, model: Option<&str>) -> crate::AgentConfig {
        crate::AgentConfig {
            name: name.into(),
            mode: crate::AgentMode::All,
            description: Default::default(),
            model: model.map(Into::into),
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: AHashMap::new(),
            tool_settings: crate::AgentToolSettings::default(),
            prompt: Default::default(),
        }
    }

    fn provider(api_url: &str, env_vars: &[&str], api_type: ProviderType) -> ProviderInfo {
        ProviderInfo {
            api_url: api_url.to_string(),
            env_vars: env_vars.iter().map(|value| (*value).to_string()).collect(),
            api_type,
        }
    }

    fn model_info(max_input: u32, max_output: u32) -> ModelInfo {
        ModelInfo {
            modalities: Modality::TEXT,
            max_input,
            max_output,
            temperature: Some(1.0),
            top_p: Some(0.95),
        }
    }

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
                        .position(|provider| provider.provider_key == provider_key)
                        .expect("provider key should exist") as u16,
                );
                ProviderModelSource::new(provider_idx, model_key, info)
            })
            .collect();

        ModelCatalog::build(&provider_sources, &provider_model_sources)
            .expect("catalog fixture should build")
    }

    #[test]
    fn resolves_runtime_default_when_agent_has_no_override() {
        let catalog = build_catalog(
            vec![(
                "openai",
                provider(
                    "https://api.openai.com/v1",
                    &["OPENAI_API_KEY"],
                    ProviderType::OpenAiResponses,
                ),
            )],
            vec![("openai", "gpt-4.1-mini", model_info(128_000, 16_384))],
        );
        let defaults = AgentDefaults::with_model("openai/gpt-4.1-mini");
        let agent = config_with_model("planner", None);

        let resolved = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect("runtime default should resolve");

        assert_eq!(resolved.provider(), "openai");
        assert_eq!(resolved.model(), "gpt-4.1-mini");
        assert_eq!(resolved.slash_spec(), "openai/gpt-4.1-mini");
    }

    #[test]
    fn agent_override_wins_over_runtime_default() {
        let catalog = build_catalog(
            vec![(
                "openrouter",
                provider(
                    "https://openrouter.ai/api/v1",
                    &["OPENROUTER_API_KEY"],
                    ProviderType::OpenRouter,
                ),
            )],
            vec![
                (
                    "openrouter",
                    "openai/gpt-4.1-mini",
                    model_info(128_000, 16_384),
                ),
                ("openrouter", "openai/gpt-4o", model_info(128_000, 16_384)),
            ],
        );
        let defaults = AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini");
        let agent = config_with_model("planner", Some("openrouter/openai/gpt-4o"));

        let resolved = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect("override should resolve");

        assert_eq!(resolved.provider(), "openrouter");
        assert_eq!(resolved.model(), "openai/gpt-4o");
    }

    #[test]
    fn malformed_agent_override_does_not_fall_back_to_defaults() {
        let catalog = build_catalog(
            vec![(
                "openai",
                provider(
                    "https://api.openai.com/v1",
                    &["OPENAI_API_KEY"],
                    ProviderType::OpenAiResponses,
                ),
            )],
            vec![("openai", "gpt-4.1-mini", model_info(128_000, 16_384))],
        );
        let defaults = AgentDefaults::with_model("openai/gpt-4.1-mini");
        let agent = config_with_model("planner", Some("openai-only"));

        let err = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect_err("malformed override should fail");

        match err {
            ModelResolutionError::MalformedModelIdentifier {
                location, model, ..
            } => {
                assert_eq!(location, "agent override");
                assert_eq!(&*model, "openai-only");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn unknown_provider_returns_specific_error() {
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
        let defaults = AgentDefaults::default();
        let agent = config_with_model("planner", Some("anthropic/claude-3-5-sonnet"));

        let err = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect_err("missing provider should fail");

        match err {
            ModelResolutionError::UnknownProvider {
                location, provider, ..
            } => {
                assert_eq!(location, "agent override");
                assert_eq!(&*provider, "anthropic");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn unknown_model_returns_specific_error() {
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
        let defaults = AgentDefaults::default();
        let agent = config_with_model("planner", Some("openai/gpt-4.1-mini"));

        let err = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect_err("missing provider/model pair should fail");

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

    #[test]
    fn missing_agent_override_and_runtime_default_returns_dedicated_error() {
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
        let defaults = AgentDefaults::default();
        let agent = config_with_model("planner", None);

        let err = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect_err("missing effective model should fail");

        match err {
            ModelResolutionError::MissingEffectiveModel { agent } => {
                assert_eq!(&*agent, "planner");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn malformed_runtime_default_returns_clear_error() {
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
        let defaults = AgentDefaults::with_model("openai-only");
        let agent = config_with_model("planner", None);

        let err = resolve_model_with_catalog(&catalog, &defaults, &agent)
            .expect_err("malformed runtime default should fail");

        match err {
            ModelResolutionError::MalformedModelIdentifier {
                location, model, ..
            } => {
                assert_eq!(location, "runtime default");
                assert_eq!(&*model, "openai-only");
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
