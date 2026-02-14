//! Model resolver for resolving model specs into provider-specific settings using models.dev catalog.

use llm_coding_tools_models_dev::{ModelsDevCatalog, ProviderMetadata};
use std::collections::HashMap;
use std::env;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Resolved model settings computed by a [`ModelResolver`].
#[derive(Clone)]
pub struct ResolvedModel {
    /// Original model spec as requested by agent/frontmatter.
    pub spec: String,
    /// Runtime provider family used by registry model construction.
    pub runtime_provider: String,
    /// Runtime model identifier consumed by provider-specific builders.
    pub runtime_model_id: String,
    /// Runtime canonical `provider:model` spec used by `ModelConfig`.
    pub runtime_spec: String,
    /// Resolved API key, if required by the provider.
    pub api_key: Option<String>,
    /// Resolved base URL, when supported and required.
    pub base_url: Option<String>,
    /// Optional per-model timeout override.
    pub timeout: Option<Duration>,
    /// Source of the resolution result.
    pub source: ResolutionSource,
    /// Original provider ID from models.dev metadata.
    pub provider_id: String,
}

impl fmt::Debug for ResolvedModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedModel")
            .field("spec", &self.spec)
            .field("runtime_provider", &self.runtime_provider)
            .field("runtime_model_id", &self.runtime_model_id)
            .field("runtime_spec", &self.runtime_spec)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("source", &self.source)
            .field("provider_id", &self.provider_id)
            .finish()
    }
}

/// Tracks how a resolved model was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSource {
    /// Explicit override provided (e.g., API key/base URL).
    ExplicitOverride,
    /// Resolved via models.dev catalog.
    ModelsDev,
    /// Fallback behavior when catalog is unavailable.
    Fallback,
}

/// Per-provider overrides for API key/base URL resolution.
#[derive(Clone, Default)]
pub struct ProviderOverride {
    /// Explicit API key override for this provider.
    pub api_key: Option<String>,
    /// Explicit base URL override for this provider.
    pub base_url: Option<String>,
    /// Explicit endpoint env var name for base URL lookup.
    pub endpoint_env: Option<String>,
}

impl fmt::Debug for ProviderOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderOverride")
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("base_url", &self.base_url)
            .field("endpoint_env", &self.endpoint_env)
            .finish()
    }
}

/// Overrides keyed by provider ID with optional default provider preference.
#[derive(Clone, Default)]
pub struct ProviderOverrides {
    /// Preferred provider ID for ambiguous model IDs.
    default_provider: Option<String>,
    /// Per-provider override entries.
    providers: HashMap<String, ProviderOverride>,
}

impl fmt::Debug for ProviderOverrides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderOverrides")
            .field("default_provider", &self.default_provider)
            .field("providers", &self.providers)
            .finish()
    }
}

impl ProviderOverrides {
    /// Create an empty override set.
    ///
    /// Returns: an empty [`ProviderOverrides`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the default provider preference.
    ///
    /// Parameters:
    /// - `provider`: provider ID to prefer for ambiguous model IDs.
    ///
    /// Returns: updated [`ProviderOverrides`].
    pub fn with_default_provider(mut self, provider: impl Into<String>) -> Self {
        self.default_provider = Some(provider.into());
        self
    }

    /// Insert a provider override.
    ///
    /// Parameters:
    /// - `provider`: provider ID to override.
    /// - `value`: override settings for that provider.
    ///
    /// Returns: updated [`ProviderOverrides`].
    pub fn insert_override(mut self, provider: impl Into<String>, value: ProviderOverride) -> Self {
        self.providers.insert(provider.into(), value);
        self
    }

    /// Returns an override entry for a provider, if configured.
    ///
    /// Parameters:
    /// - `provider`: provider ID to look up.
    ///
    /// Returns: `Some(&ProviderOverride)` when present.
    pub fn get_override(&self, provider: &str) -> Option<&ProviderOverride> {
        self.providers.get(provider)
    }
}

/// Errors produced by model resolution.
#[derive(Debug, Clone)]
pub enum ModelResolveError {
    /// Model ID not found in catalog.
    NotFound(String),
    /// Provider prefix is unknown.
    UnknownProvider(String),
    /// Provider is not supported by ModelConfig/build_model_with_config.
    UnsupportedProvider {
        /// Provider ID.
        provider: String,
        /// NPM package name, if available.
        npm: Option<String>,
    },
    /// Provider exists but does not support the requested model.
    ModelNotSupported {
        /// Provider ID.
        provider: String,
        /// Model ID that is not supported.
        model_id: String,
    },
    /// Model ID maps to multiple providers and cannot be disambiguated.
    Ambiguous {
        /// Model ID that is ambiguous.
        model_id: String,
        /// List of provider IDs that support this model.
        providers: Vec<String>,
        /// The default provider preference, if set.
        default_provider: Option<String>,
    },
    /// No API key env var candidate found.
    MissingApiKeyEnv {
        /// Provider ID.
        provider: String,
    },
    /// API key env var missing or empty.
    MissingApiKeyValue {
        /// Provider ID.
        provider: String,
        /// Environment variable name.
        env: String,
    },
    /// Base URL is required but missing.
    MissingBaseUrl {
        /// Provider ID.
        provider: String,
    },
    /// Base URL was provided for a provider that does not support it.
    BaseUrlUnsupported {
        /// Provider ID.
        provider: String,
    },
}

impl fmt::Display for ModelResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(model) => write!(f, "model '{model}' not found"),
            Self::UnknownProvider(provider) => write!(f, "unknown provider '{provider}'"),
            Self::UnsupportedProvider { provider, npm } => write!(
                f,
                "unsupported provider '{provider}' (npm: {:?})",
                npm.as_deref()
            ),
            Self::ModelNotSupported { provider, model_id } => write!(
                f,
                "model '{model_id}' is not supported by provider '{provider}'"
            ),
            Self::Ambiguous {
                model_id,
                providers,
                default_provider,
            } => write!(
                f,
                "model '{model_id}' is ambiguous across providers: {:?} (default: {:?})",
                providers, default_provider
            ),
            Self::MissingApiKeyEnv { provider } => {
                write!(f, "no API key env var candidate for provider '{provider}'")
            }
            Self::MissingApiKeyValue { provider, env } => write!(
                f,
                "API key env var '{env}' missing for provider '{provider}'"
            ),
            Self::MissingBaseUrl { provider } => write!(
                f,
                "missing base URL for provider '{provider}' (can be supplied via override or env var)"
            ),
            Self::BaseUrlUnsupported { provider } => write!(
                f,
                "base URL overrides are unsupported for provider '{provider}'"
            ),
        }
    }
}

impl std::error::Error for ModelResolveError {}

/// Resolves model specs into per-provider settings.
pub trait ModelResolver: Send + Sync {
    /// Resolves the provided model spec.
    ///
    /// Parameters:
    /// - `model_spec`: input spec in `provider:model` or `model` format.
    ///
    /// Returns: resolved model settings or a [`ModelResolveError`].
    fn resolve(&self, model_spec: &str) -> Result<ResolvedModel, ModelResolveError>;
}

/// Shared, cloneable resolver handle for registry defaults and builder wiring.
pub type SharedModelResolver = Arc<dyn ModelResolver + Send + Sync>;

/// models.dev-backed resolver implementation.
#[derive(Debug, Clone)]
pub struct ModelsDevResolver {
    /// Optional catalog for models.dev lookups.
    catalog: Option<ModelsDevCatalog>,
    /// Override values for resolution behavior.
    overrides: ProviderOverrides,
}

impl ModelsDevResolver {
    /// Create a resolver with optional catalog and overrides.
    ///
    /// Parameters:
    /// - `catalog`: optional models.dev catalog.
    /// - `overrides`: provider override configuration.
    ///
    /// Returns: a new [`ModelsDevResolver`].
    pub fn new(catalog: Option<ModelsDevCatalog>, overrides: ProviderOverrides) -> Self {
        Self { catalog, overrides }
    }
}

struct ParsedModelSpec<'a> {
    requested_spec: &'a str,
    explicit_provider: Option<&'a str>,
    model_id: &'a str,
}

fn parse_model_spec<'a>(model_spec: &'a str) -> ParsedModelSpec<'a> {
    let colon_pos = model_spec.find(':');
    let slash_pos = model_spec.find('/');

    let use_colon = match (colon_pos, slash_pos) {
        (Some(c), Some(s)) => c < s,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => false,
    };

    if use_colon {
        if let Some((provider, model_id)) = model_spec.split_once(':')
            && !provider.is_empty()
            && !model_id.is_empty()
        {
            return ParsedModelSpec {
                requested_spec: model_spec,
                explicit_provider: Some(provider),
                model_id,
            };
        }
    } else if let Some((provider, model_id)) = model_spec.split_once('/')
        && !provider.is_empty()
        && !model_id.is_empty()
    {
        return ParsedModelSpec {
            requested_spec: model_spec,
            explicit_provider: Some(provider),
            model_id,
        };
    }

    ParsedModelSpec {
        requested_spec: model_spec,
        explicit_provider: None,
        model_id: model_spec,
    }
}

fn infer_runtime_from_raw_spec(model_spec: &str) -> (String, String, String) {
    let parsed = parse_model_spec(model_spec);
    let provider = parsed.explicit_provider.unwrap_or("openai");
    let model_id = parsed.model_id;

    let runtime_provider = match provider {
        "anthropic" => "anthropic",
        "google" => "google",
        "groq" => "groq",
        "mistral" => "mistral",
        "cohere" => "cohere",
        "ollama" => "ollama",
        "openrouter" => "openrouter",
        "huggingface" => "huggingface",
        _ => "openai",
    };

    let runtime_spec = format!("{}:{}", runtime_provider, model_id);
    (
        runtime_provider.to_string(),
        model_id.to_string(),
        runtime_spec,
    )
}

impl ModelResolver for ModelsDevResolver {
    fn resolve(&self, model_spec: &str) -> Result<ResolvedModel, ModelResolveError> {
        let Some(catalog) = &self.catalog else {
            let (runtime_provider, runtime_model_id, runtime_spec) =
                infer_runtime_from_raw_spec(model_spec);
            return Ok(ResolvedModel {
                spec: model_spec.to_string(),
                runtime_provider,
                runtime_model_id,
                runtime_spec,
                api_key: None,
                base_url: None,
                timeout: None,
                source: ResolutionSource::Fallback,
                provider_id: String::new(),
            });
        };

        let parsed = parse_model_spec(model_spec);
        let provider_prefix = parsed.explicit_provider;
        let model_id = parsed.model_id;

        if let Some(provider_id) = provider_prefix {
            let provider = catalog
                .get_provider(provider_id)
                .ok_or_else(|| ModelResolveError::UnknownProvider(provider_id.to_string()))?;
            let providers = catalog.resolve_provider_for_model(model_id).unwrap_or(&[]);
            if !providers.iter().any(|id| id == provider_id) {
                return Err(ModelResolveError::ModelNotSupported {
                    provider: provider_id.to_string(),
                    model_id: model_id.to_string(),
                });
            }

            return resolve_provider_model(
                provider,
                parsed.requested_spec,
                model_id,
                true,
                &self.overrides,
            );
        }

        let providers = catalog.resolve_provider_for_model(model_id).unwrap_or(&[]);
        if providers.is_empty() {
            return Err(ModelResolveError::NotFound(model_id.to_string()));
        }

        let provider_id = if providers.len() == 1 {
            // Single provider is not ambiguous - select it directly
            providers.first().map(String::as_str).unwrap()
        } else if let Some(default_provider) = self.overrides.default_provider.as_deref() {
            if let Some(found) = providers.iter().find(|id| id.as_str() == default_provider) {
                found.as_str()
            } else {
                return Err(ModelResolveError::Ambiguous {
                    model_id: model_id.to_string(),
                    providers: providers.to_vec(),
                    default_provider: Some(default_provider.to_string()),
                });
            }
        } else if let Some(found) = providers.iter().find(|id| id.as_str() == "openai") {
            found.as_str()
        } else {
            return Err(ModelResolveError::Ambiguous {
                model_id: model_id.to_string(),
                providers: providers.to_vec(),
                default_provider: None,
            });
        };

        let provider = catalog
            .get_provider(provider_id)
            .ok_or_else(|| ModelResolveError::UnknownProvider(provider_id.to_string()))?;
        resolve_provider_model(
            provider,
            parsed.requested_spec,
            model_id,
            false,
            &self.overrides,
        )
    }
}

fn resolve_provider_model(
    provider: &ProviderMetadata,
    requested_spec: &str,
    model_id: &str,
    explicit_provider: bool,
    overrides: &ProviderOverrides,
) -> Result<ResolvedModel, ModelResolveError> {
    let (serdes_provider, supports_base_url, requires_api_key, requires_base_url) =
        match provider.npm.as_deref() {
            Some("@ai-sdk/openai") => ("openai", true, true, false),
            Some("@ai-sdk/openai-compatible") => ("openai", true, true, true),
            Some("@ai-sdk/anthropic") => ("anthropic", true, true, false),
            Some("@ai-sdk/groq") => ("groq", false, true, false),
            Some("@ai-sdk/mistral") => ("mistral", true, true, false),
            Some("@ai-sdk/google") => ("google", true, true, false),
            Some("@ai-sdk/cohere") => ("cohere", true, true, false),
            Some("@ai-sdk/ollama") => ("ollama", true, false, false),
            Some("@ai-sdk/openrouter") => ("openrouter", false, true, false),
            Some("@ai-sdk/huggingface") => ("huggingface", true, false, false),
            Some("@ai-sdk/azure")
            | Some("@ai-sdk/google-vertex")
            | Some("@ai-sdk/google-vertex/anthropic") => {
                return Err(ModelResolveError::UnsupportedProvider {
                    provider: provider.id.clone(),
                    npm: provider.npm.clone(),
                });
            }
            Some(_) | None => {
                return Err(ModelResolveError::UnsupportedProvider {
                    provider: provider.id.clone(),
                    npm: provider.npm.clone(),
                });
            }
        };

    let override_entry = overrides.providers.get(&provider.id);

    let (api_key, used_api_override) = if let Some(api_key) = override_entry
        .and_then(|cfg| cfg.api_key.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        (Some(api_key.to_string()), true)
    } else if !requires_api_key {
        (None, false)
    } else {
        let env_name = provider
            .env
            .iter()
            .map(|value| value.as_str())
            .find(|name| {
                let upper = name.to_ascii_uppercase();
                let is_key_like =
                    upper.contains("API_KEY") || upper.contains("TOKEN") || upper.ends_with("_KEY");
                let is_endpoint = upper.contains("ENDPOINT")
                    || upper.contains("BASE_URL")
                    || upper.contains("BASEURL")
                    || upper.contains("API_URL");
                let is_misc = upper.contains("PROJECT")
                    || upper.contains("LOCATION")
                    || upper.contains("CREDENTIALS")
                    || upper.contains("RESOURCE");
                is_key_like && !is_endpoint && !is_misc
            })
            .ok_or_else(|| ModelResolveError::MissingApiKeyEnv {
                provider: provider.id.clone(),
            })?;

        let value = env::var(env_name).map_err(|_| ModelResolveError::MissingApiKeyValue {
            provider: provider.id.clone(),
            env: env_name.to_string(),
        })?;
        if value.trim().is_empty() {
            return Err(ModelResolveError::MissingApiKeyValue {
                provider: provider.id.clone(),
                env: env_name.to_string(),
            });
        }
        (Some(value), false)
    };

    let (base_url, used_base_override) = if let Some(base_url) = override_entry
        .and_then(|cfg| cfg.base_url.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        if !supports_base_url {
            return Err(ModelResolveError::BaseUrlUnsupported {
                provider: provider.id.clone(),
            });
        }
        (Some(base_url.to_string()), true)
    } else if !supports_base_url {
        (None, false)
    } else {
        let endpoint_env = override_entry
            .and_then(|cfg| cfg.endpoint_env.as_deref())
            .or_else(|| {
                provider
                    .env
                    .iter()
                    .map(|value| value.as_str())
                    .find(|name| {
                        let upper = name.to_ascii_uppercase();
                        let is_endpoint = upper.contains("ENDPOINT")
                            || upper.contains("BASE_URL")
                            || upper.contains("BASEURL")
                            || upper.contains("API_URL");
                        let is_key_like = upper.contains("API_KEY")
                            || upper.contains("TOKEN")
                            || upper.ends_with("_KEY");
                        is_endpoint && !is_key_like
                    })
            });

        let env_value = endpoint_env
            .and_then(|env_name| env::var(env_name).ok())
            .filter(|value| !value.trim().is_empty());

        if let Some(value) = env_value {
            (Some(value), false)
        } else if requires_base_url {
            let api = provider
                .api
                .clone()
                .ok_or_else(|| ModelResolveError::MissingBaseUrl {
                    provider: provider.id.clone(),
                })?;
            (Some(api), false)
        } else {
            (None, false)
        }
    };

    let runtime_spec = format!("{}:{}", serdes_provider, model_id);
    let source = if explicit_provider || used_api_override || used_base_override {
        ResolutionSource::ExplicitOverride
    } else {
        ResolutionSource::ModelsDev
    };

    Ok(ResolvedModel {
        spec: requested_spec.to_string(),
        runtime_provider: serdes_provider.to_string(),
        runtime_model_id: model_id.to_string(),
        runtime_spec,
        api_key,
        base_url,
        timeout: None,
        source,
        provider_id: provider.id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn catalog_from_json(json: &str) -> ModelsDevCatalog {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let path = temp.path().join("api.json");
        std::fs::write(&path, json).expect("write api.json");
        ModelsDevCatalog::from_local_api_json(&path).expect("catalog")
    }

    #[test]
    fn prefixed_resolution_success() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("alpha:m1").expect("resolve");
        assert_eq!(resolved.spec, "alpha:m1");
        assert_eq!(resolved.runtime_spec, "openai:m1");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "m1");
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn prefixed_unknown_provider_errors() {
        let resolver = ModelsDevResolver::new(
            Some(catalog_from_json(r#"{"providers":{}}"#)),
            ProviderOverrides::new(),
        );
        let err = resolver
            .resolve("unknown:m1")
            .expect_err("unknown provider");
        assert!(matches!(err, ModelResolveError::UnknownProvider(_)));
    }

    #[test]
    fn unique_provider_resolution_success() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("m1").expect("resolve");
        assert_eq!(resolved.spec, "m1");
        assert_eq!(resolved.runtime_spec, "openai:m1");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "m1");
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn ambiguous_model_without_openai_errors() {
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/anthropic","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}},"beta":{"id":"beta","npm":"@ai-sdk/mistral","api":null,"env":["BETA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver.resolve("m1").expect_err("ambiguous");
        assert!(matches!(err, ModelResolveError::Ambiguous { .. }));
    }

    #[test]
    fn fallback_when_catalog_missing() {
        let resolver = ModelsDevResolver::new(None, ProviderOverrides::new());
        let resolved = resolver.resolve("openai:gpt-4o").expect("fallback");
        assert_eq!(resolved.spec, "openai:gpt-4o");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "gpt-4o");
        assert_eq!(resolved.runtime_spec, "openai:gpt-4o");
        assert!(matches!(resolved.source, ResolutionSource::Fallback));
    }

    #[test]
    fn fallback_preserves_provider_id() {
        let resolver = ModelsDevResolver::new(None, ProviderOverrides::new());
        let resolved = resolver.resolve("any:model").expect("fallback");
        assert_eq!(resolved.provider_id, "");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "model");
    }

    #[test]
    fn api_key_resolution_success() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("alpha:m1").expect("resolve");
        assert_eq!(resolved.api_key.as_deref(), Some("key"));
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn api_key_from_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let overrides = ProviderOverrides::new().insert_override(
            "alpha",
            ProviderOverride {
                api_key: Some("override_key".to_string()),
                base_url: None,
                endpoint_env: None,
            },
        );
        let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), overrides);
        let resolved = resolver.resolve("alpha:m1").expect("resolve");
        assert_eq!(resolved.api_key.as_deref(), Some("override_key"));
    }

    #[test]
    fn missing_api_key_env_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver
            .resolve("alpha:m1")
            .expect_err("missing api key env");
        assert!(matches!(err, ModelResolveError::MissingApiKeyValue { .. }));
    }

    #[test]
    fn ollama_no_api_key_required() {
        let json = r#"{"providers":{"ollama":{"id":"ollama","npm":"@ai-sdk/ollama","api":null,"env":[],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("ollama:m1").expect("resolve");
        assert_eq!(resolved.api_key, None);
        assert_eq!(resolved.base_url, None);
    }

    #[test]
    fn provider_with_unsupported_env_rejected() {
        let json = r#"{"providers":{"azure":{"id":"azure","npm":"@ai-sdk/azure","api":null,"env":["AZURE_API_KEY","AZURE_PROJECT"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver
            .resolve("azure:m1")
            .expect_err("unsupported provider");
        assert!(matches!(err, ModelResolveError::UnsupportedProvider { .. }));
    }

    #[test]
    fn provider_with_extra_non_key_env_resolves_successfully() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("OPENAI_API_KEY", "key") };
        // Provider has extra env vars (PROJECT, REGION) but should still resolve via API key
        let json = r#"{"providers":{"openai":{"id":"openai","npm":"@ai-sdk/openai","api":null,"env":["OPENAI_API_KEY","OPENAI_PROJECT","OPENAI_REGION"],"models":{"gpt-4":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("openai:gpt-4").expect("resolve");
        assert_eq!(resolved.spec, "openai:gpt-4");
        assert_eq!(resolved.runtime_spec, "openai:gpt-4");
        assert_eq!(resolved.api_key.as_deref(), Some("key"));
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn provider_without_npm_rejected() {
        let json = r#"{"providers":{"custom":{"id":"custom","npm":null,"api":null,"env":["CUSTOM_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver
            .resolve("custom:m1")
            .expect_err("unsupported provider");
        assert!(matches!(err, ModelResolveError::UnsupportedProvider { .. }));
    }

    #[test]
    fn base_url_resolution_success_from_api() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ROUTER_API_KEY", "key") };
        let json = r#"{"providers":{"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":"https://example.com","env":["ROUTER_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("router:m1").expect("resolve");
        assert_eq!(resolved.base_url.as_deref(), Some("https://example.com"));
        unsafe { std::env::remove_var("ROUTER_API_KEY") };
    }

    #[test]
    fn base_url_precedence_override_then_env_then_api() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ROUTER_API_KEY", "key") };
        unsafe { std::env::set_var("ROUTER_ENDPOINT", "https://env.example.com") };
        let json = r#"{"providers":{"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":"https://api.example.com","env":["ROUTER_API_KEY","ROUTER_ENDPOINT"],"models":{"m1":{}}}}}"#;
        let overrides = ProviderOverrides::new().insert_override(
            "router",
            ProviderOverride {
                api_key: None,
                base_url: Some("https://override.example.com".to_string()),
                endpoint_env: None,
            },
        );
        let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), overrides);
        let resolved = resolver.resolve("router:m1").expect("resolve");
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("https://override.example.com")
        );
        unsafe { std::env::remove_var("ROUTER_API_KEY") };
        unsafe { std::env::remove_var("ROUTER_ENDPOINT") };
    }

    #[test]
    fn base_url_precedence_env_over_api() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ROUTER_API_KEY", "key") };
        unsafe { std::env::set_var("ROUTER_ENDPOINT", "https://env.example.com") };
        let json = r#"{"providers":{"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":"https://api.example.com","env":["ROUTER_API_KEY","ROUTER_ENDPOINT"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("router:m1").expect("resolve");
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("https://env.example.com")
        );
        unsafe { std::env::remove_var("ROUTER_API_KEY") };
        unsafe { std::env::remove_var("ROUTER_ENDPOINT") };
    }

    #[test]
    fn base_url_endpoint_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ROUTER_API_KEY", "key") };
        unsafe { std::env::set_var("CUSTOM_ENDPOINT", "https://custom.example.com") };
        let json = r#"{"providers":{"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":"https://api.example.com","env":["ROUTER_API_KEY","ROUTER_ENDPOINT"],"models":{"m1":{}}}}}"#;
        let overrides = ProviderOverrides::new().insert_override(
            "router",
            ProviderOverride {
                api_key: None,
                base_url: None,
                endpoint_env: Some("CUSTOM_ENDPOINT".to_string()),
            },
        );
        let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), overrides);
        let resolved = resolver.resolve("router:m1").expect("resolve");
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("https://custom.example.com")
        );
        unsafe { std::env::remove_var("ROUTER_API_KEY") };
        unsafe { std::env::remove_var("CUSTOM_ENDPOINT") };
    }

    #[test]
    fn missing_base_url_errors_when_required() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("ROUTER_ENDPOINT");
            std::env::remove_var("ROUTER_BASE_URL");
            std::env::remove_var("ROUTER_API_URL");
            std::env::set_var("ROUTER_API_KEY", "key")
        };
        let json = r#"{"providers":{"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":null,"env":["ROUTER_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver.resolve("router:m1").expect_err("missing base url");
        assert!(matches!(err, ModelResolveError::MissingBaseUrl { .. }));
        unsafe { std::env::remove_var("ROUTER_API_KEY") };
    }

    #[test]
    fn base_url_unsupported_for_groq_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("GROQ_API_KEY", "key") };
        let json = r#"{"providers":{"groq":{"id":"groq","npm":"@ai-sdk/groq","api":null,"env":["GROQ_API_KEY"],"models":{"m1":{}}}}}"#;
        let overrides = ProviderOverrides::new().insert_override(
            "groq",
            ProviderOverride {
                api_key: None,
                base_url: Some("https://example.com".to_string()),
                endpoint_env: None,
            },
        );
        let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), overrides);
        let err = resolver
            .resolve("groq:m1")
            .expect_err("base_url unsupported");
        assert!(matches!(err, ModelResolveError::BaseUrlUnsupported { .. }));
        unsafe { std::env::remove_var("GROQ_API_KEY") };
    }

    #[test]
    fn model_not_supported_by_provider_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver
            .resolve("alpha:m2")
            .expect_err("model not supported");
        assert!(matches!(err, ModelResolveError::ModelNotSupported { .. }));
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn model_not_found_errors() {
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver.resolve("unknown_model").expect_err("not found");
        assert!(matches!(err, ModelResolveError::NotFound(_)));
    }

    #[test]
    fn explicit_override_sets_source() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("alpha:m1").expect("resolve");
        assert_eq!(resolved.source, ResolutionSource::ExplicitOverride);
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn models_dev_source_set_for_implicit_resolution() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("OPENAI_API_KEY", "key") };
        let json = r#"{"providers":{"openai":{"id":"openai","npm":"@ai-sdk/openai","api":null,"env":["OPENAI_API_KEY"],"models":{"gpt-4":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("gpt-4").expect("resolve");
        assert_eq!(resolved.source, ResolutionSource::ModelsDev);
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn provider_id_preserved_for_openai_compatible() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ROUTER_API_KEY", "key") };
        let json = r#"{"providers":{"router":{"id":"router","npm":"@ai-sdk/openai-compatible","api":"https://example.com","env":["ROUTER_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("router:m1").expect("resolve");
        assert_eq!(resolved.provider_id, "router");
        assert_eq!(resolved.spec, "router:m1");
        assert_eq!(resolved.runtime_spec, "openai:m1");
        assert_eq!(resolved.runtime_provider, "openai");
        unsafe { std::env::remove_var("ROUTER_API_KEY") };
    }

    #[test]
    fn default_provider_disambiguates() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        unsafe { std::env::set_var("BETA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/anthropic","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}},"beta":{"id":"beta","npm":"@ai-sdk/mistral","api":null,"env":["BETA_API_KEY"],"models":{"m1":{}}}}}"#;
        let overrides = ProviderOverrides::new().with_default_provider("beta");
        let resolver = ModelsDevResolver::new(Some(catalog_from_json(json)), overrides);
        let resolved = resolver.resolve("m1").expect("resolve");
        assert_eq!(resolved.spec, "m1");
        assert_eq!(resolved.runtime_spec, "mistral:m1");
        assert_eq!(resolved.runtime_provider, "mistral");
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
        unsafe { std::env::remove_var("BETA_API_KEY") };
    }

    #[test]
    fn provider_overrides_empty_by_default() {
        let overrides = ProviderOverrides::new();
        assert!(overrides.providers.is_empty());
        assert!(overrides.default_provider.is_none());
    }

    #[test]
    fn token_based_api_key_recognized() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ANTHROPIC_TOKEN", "token") };
        let json = r#"{"providers":{"anthropic":{"id":"anthropic","npm":"@ai-sdk/anthropic","api":null,"env":["ANTHROPIC_TOKEN"],"models":{"claude-3":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("anthropic:claude-3").expect("resolve");
        assert_eq!(resolved.api_key.as_deref(), Some("token"));
        unsafe { std::env::remove_var("ANTHROPIC_TOKEN") };
    }

    #[test]
    fn suffix_key_based_api_key_recognized() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("MISTRAL_KEY", "key") };
        let json = r#"{"providers":{"mistral":{"id":"mistral","npm":"@ai-sdk/mistral","api":null,"env":["MISTRAL_KEY"],"models":{"mistral-large":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("mistral:mistral-large").expect("resolve");
        assert_eq!(resolved.api_key.as_deref(), Some("key"));
        unsafe { std::env::remove_var("MISTRAL_KEY") };
    }

    #[test]
    fn anthropic_base_url_from_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", "key") };
        unsafe { std::env::set_var("ANTHROPIC_BASE_URL", "https://custom.anthropic.com") };
        let json = r#"{"providers":{"anthropic":{"id":"anthropic","npm":"@ai-sdk/anthropic","api":"https://api.anthropic.com","env":["ANTHROPIC_API_KEY","ANTHROPIC_BASE_URL"],"models":{"claude-3":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("anthropic:claude-3").expect("resolve");
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("https://custom.anthropic.com")
        );
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
        unsafe { std::env::remove_var("ANTHROPIC_BASE_URL") };
    }

    #[test]
    fn google_provider_supported() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("GOOGLE_API_KEY", "key") };
        let json = r#"{"providers":{"google":{"id":"google","npm":"@ai-sdk/google","api":null,"env":["GOOGLE_API_KEY"],"models":{"gemini-pro":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("google:gemini-pro").expect("resolve");
        assert_eq!(resolved.spec, "google:gemini-pro");
        assert_eq!(resolved.runtime_spec, "google:gemini-pro");
        assert_eq!(resolved.runtime_provider, "google");
        assert_eq!(resolved.api_key.as_deref(), Some("key"));
        unsafe { std::env::remove_var("GOOGLE_API_KEY") };
    }

    #[test]
    fn google_vertex_provider_rejected() {
        let json = r#"{"providers":{"vertex":{"id":"vertex","npm":"@ai-sdk/google-vertex","api":null,"env":["VERTEX_PROJECT"],"models":{"gemini-pro":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver
            .resolve("vertex:gemini-pro")
            .expect_err("unsupported provider");
        assert!(matches!(err, ModelResolveError::UnsupportedProvider { .. }));
    }

    #[test]
    fn empty_api_key_env_treated_as_missing() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let err = resolver.resolve("alpha:m1").expect_err("empty api key");
        assert!(matches!(err, ModelResolveError::MissingApiKeyValue { .. }));
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn spec_with_colon_in_model_id_handles_correctly() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let json = r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"model:v1":{}}}}}"#;
        let resolver =
            ModelsDevResolver::new(Some(catalog_from_json(json)), ProviderOverrides::new());
        let resolved = resolver.resolve("alpha:model:v1").expect("resolve");
        assert_eq!(resolved.spec, "alpha:model:v1");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "model:v1");
        assert_eq!(resolved.runtime_spec, "openai:model:v1");
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn fallback_populates_runtime_fields() {
        let resolver = ModelsDevResolver::new(None, ProviderOverrides::new());
        let resolved = resolver
            .resolve("synthetic/hf:zai-org/GLM-4.7")
            .expect("fallback");
        assert_eq!(resolved.spec, "synthetic/hf:zai-org/GLM-4.7");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "hf:zai-org/GLM-4.7");
        assert_eq!(resolved.runtime_spec, "openai:hf:zai-org/GLM-4.7");
        assert!(matches!(resolved.source, ResolutionSource::Fallback));
    }

    #[test]
    fn fallback_colon_form_with_slash_model_id_populates_runtime_fields() {
        let resolver = ModelsDevResolver::new(None, ProviderOverrides::new());
        let resolved = resolver
            .resolve("huggingface:tiiuae/falcon-7b")
            .expect("fallback");
        assert_eq!(resolved.spec, "huggingface:tiiuae/falcon-7b");
        assert_eq!(resolved.runtime_provider, "huggingface");
        assert_eq!(resolved.runtime_model_id, "tiiuae/falcon-7b");
        assert_eq!(resolved.runtime_spec, "huggingface:tiiuae/falcon-7b");
        assert!(matches!(resolved.source, ResolutionSource::Fallback));
    }

    #[test]
    fn slash_form_unknown_provider_errors() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("ALPHA_API_KEY", "key") };
        let resolver = ModelsDevResolver::new(
            Some(catalog_from_json(
                r#"{"providers":{"alpha":{"id":"alpha","npm":"@ai-sdk/openai","api":null,"env":["ALPHA_API_KEY"],"models":{"m1":{}}}}}"#,
            )),
            ProviderOverrides::new(),
        );
        let err = resolver
            .resolve("unknown/m1")
            .expect_err("unknown slash provider");
        assert!(
            matches!(err, ModelResolveError::UnknownProvider(provider) if provider == "unknown")
        );
        unsafe { std::env::remove_var("ALPHA_API_KEY") };
    }

    #[test]
    fn slash_form_with_colon_model_id_resolves() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("SYNTH_API_KEY", "key") };
        let resolver = ModelsDevResolver::new(
            Some(catalog_from_json(
                r#"{"providers":{"synthetic":{"id":"synthetic","npm":"@ai-sdk/openai-compatible","api":"https://api.synthetic/v1","env":["SYNTH_API_KEY"],"models":{"hf:zai-org/GLM-4.7":{}}}}}"#,
            )),
            ProviderOverrides::new(),
        );
        let resolved = resolver
            .resolve("synthetic/hf:zai-org/GLM-4.7")
            .expect("resolve");
        assert_eq!(resolved.spec, "synthetic/hf:zai-org/GLM-4.7");
        assert_eq!(resolved.runtime_provider, "openai");
        assert_eq!(resolved.runtime_model_id, "hf:zai-org/GLM-4.7");
        unsafe { std::env::remove_var("SYNTH_API_KEY") };
    }

    #[test]
    fn colon_form_with_slash_model_id_still_resolves() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe { std::env::set_var("HF_API_KEY", "key") };
        let resolver = ModelsDevResolver::new(
            Some(catalog_from_json(
                r#"{"providers":{"huggingface":{"id":"huggingface","npm":"@ai-sdk/openai-compatible","api":"https://api.hf/v1","env":["HF_API_KEY"],"models":{"tiiuae/falcon-7b":{}}}}}"#,
            )),
            ProviderOverrides::new(),
        );
        let resolved = resolver
            .resolve("huggingface:tiiuae/falcon-7b")
            .expect("resolve");
        assert_eq!(resolved.spec, "huggingface:tiiuae/falcon-7b");
        assert_eq!(resolved.runtime_model_id, "tiiuae/falcon-7b");
        unsafe { std::env::remove_var("HF_API_KEY") };
    }
}
