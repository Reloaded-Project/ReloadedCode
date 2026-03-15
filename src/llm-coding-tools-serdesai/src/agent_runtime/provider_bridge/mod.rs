//! Map catalog [`ProviderType`] values to concrete SerdesAI model constructors.

#![cfg_attr(not(test), allow(dead_code))]

use llm_coding_tools_agents::ResolvedModel;
use llm_coding_tools_core::{
    CredentialLookup,
    models::{ModelCatalog, ProviderType},
};
use serdes_ai_models::{BoxedModel, Model as SerdesModel, ModelError};
use std::sync::Arc;

const COHERE_BASE_URL: &str = "https://api.cohere.ai/v2";
const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
const OPENAI_COMPATIBLE_PROVIDER: &str = "openai";
const AWS_ACCESS_KEY_ID_ENV_VAR: &str = "AWS_ACCESS_KEY_ID";
const AWS_SECRET_ACCESS_KEY_ENV_VAR: &str = "AWS_SECRET_ACCESS_KEY";
const AWS_SESSION_TOKEN_ENV_VAR: &str = "AWS_SESSION_TOKEN";
const AWS_REGION_ENV_VAR: &str = "AWS_REGION";
const AWS_DEFAULT_REGION_ENV_VAR: &str = "AWS_DEFAULT_REGION";

/// Concrete SerdesAI model prepared from catalog metadata.
#[derive(Clone)]
pub(super) struct ResolvedSerdesModel {
    /// Concrete model instance ready for [`serdes_ai::AgentBuilder::from_arc`].
    pub(super) model: BoxedModel,
    /// Normalized `provider:model` debug spec used by tests and diagnostics.
    pub(super) spec: Box<str>,
}

impl ResolvedSerdesModel {
    #[inline]
    fn new<M>(provider_name: &'static str, model_name: &str, model: M) -> Self
    where
        M: SerdesModel + 'static,
    {
        let mut spec = String::with_capacity(provider_name.len() + model_name.len() + 1);
        spec.push_str(provider_name);
        spec.push(':');
        spec.push_str(model_name);
        Self {
            model: Arc::new(model),
            spec: spec.into_boxed_str(),
        }
    }
}

/// Normalizes an API URL from the catalog by trimming whitespace and trailing slashes.
///
/// Returns `None` if the result is empty, allowing callers to treat missing/empty URLs
/// uniformly as "use the provider's default endpoint".
#[inline]
fn normalized_api_url(api_url: &str) -> Option<&str> {
    let trimmed = api_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Checks if an environment variable name represents an authentication credential.
///
/// Providers list environment variable names in their catalog entry. This predicate
/// identifies which ones contain secrets like API keys or tokens, used to extract
/// credentials for model construction.
#[inline]
fn is_credential_env_var(env_var: &str) -> bool {
    env_var.ends_with("_API_KEY")
        || env_var.ends_with("_ACCESS_TOKEN")
        || env_var.ends_with("_TOKEN")
}

/// Finds the first environment variable matching a predicate that has a non-empty value.
///
/// The catalog lists possible environment variable names for a provider. This function
/// searches through them in order and returns the resolved value of the first one that
/// both matches the predicate and is actually set.
fn first_matching_env_value<P>(
    credentials: &impl CredentialLookup,
    env_vars: &[&str],
    mut predicate: P,
) -> Option<String>
where
    P: FnMut(&str) -> bool,
{
    env_vars.iter().copied().find_map(|env_var| {
        if !predicate(env_var) {
            return None;
        }
        credentials.resolve(env_var)
    })
}

/// Formats a comma-separated list of environment variable names matching a predicate.
///
/// Used in error messages to tell users which environment variables they can set
/// to provide a required value (credential, endpoint, etc.).
fn matching_env_names<P>(env_vars: &[&str], mut predicate: P) -> String
where
    P: FnMut(&str) -> bool,
{
    let mut names = String::new();
    for env_var in env_vars
        .iter()
        .copied()
        .filter(|env_var| predicate(env_var))
    {
        if !names.is_empty() {
            names.push_str(", ");
        }
        names.push_str(env_var);
    }
    if names.is_empty() {
        names.push_str("<none listed in catalog>");
    }
    names
}

/// Finds the first resolved value among explicit credential names.
fn first_resolved_name(credentials: &impl CredentialLookup, names: &[&str]) -> Option<String> {
    names
        .iter()
        .copied()
        .find_map(|name| credentials.resolve(name))
}

/// Requires an environment variable matching a predicate to have a value.
///
/// Returns the resolved value if found, otherwise returns a configuration error
/// that lists the available environment variable names for user guidance.
fn require_env_value<P>(
    credentials: &impl CredentialLookup,
    provider_key: &str,
    provider_name: &str,
    env_vars: &[&str],
    kind: &str,
    predicate: P,
) -> Result<String, ModelError>
where
    P: Copy + Fn(&str) -> bool,
{
    if let Some(value) = first_matching_env_value(credentials, env_vars, predicate) {
        return Ok(value);
    }

    Err(ModelError::configuration(format!(
        "provider `{provider_key}` mapped to serdes `{provider_name}` requires {kind}; set one of: {}",
        matching_env_names(env_vars, predicate)
    )))
}

/// Requires a specific named credential to have a value.
fn require_named_value(
    credentials: &impl CredentialLookup,
    provider_key: &str,
    provider_name: &str,
    name: &str,
    kind: &str,
) -> Result<String, ModelError> {
    if let Some(value) = credentials.resolve(name) {
        return Ok(value);
    }

    Err(ModelError::configuration(format!(
        "provider `{provider_key}` mapped to serdes `{provider_name}` requires {kind}; set `{name}`"
    )))
}

/// Creates an error for a provider whose feature flag is disabled at compile time.
#[allow(dead_code)]
#[inline]
fn feature_disabled_error(feature: &str, provider_name: &str) -> ModelError {
    ModelError::configuration(format!(
        "provider `{provider_name}` is not enabled in llm-coding-tools-serdesai; rebuild with `--features {feature}`"
    ))
}

/// Builds the concrete SerdesAI model for a validated runtime model selection.
pub(super) fn build_serdes_model(
    catalog: &ModelCatalog,
    resolved: &ResolvedModel,
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    let provider = catalog
        .lookup_provider(resolved.provider())
        .ok_or_else(|| {
            ModelError::configuration(format!(
                "effective provider `{}` disappeared from the model catalog after validation",
                resolved.provider()
            ))
        })?;
    let api_url = normalized_api_url(provider.api_url);
    let env_vars = provider.env_vars();

    match provider.api_type {
        ProviderType::Unknown => Err(ModelError::configuration(format!(
            "provider `{}` has no SerdesAI mapping because its catalog provider type is unknown",
            resolved.provider()
        ))),
        ProviderType::OpenAiCompletions => build_openai_chat(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::OpenAiResponses => build_openai_responses(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Anthropic => build_anthropic(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Google => build_google(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Groq => build_groq(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Mistral => build_mistral(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Ollama => build_ollama(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Bedrock => build_bedrock(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Azure => build_azure(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::OpenRouter => build_openrouter(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::HuggingFace => build_huggingface(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Cohere => build_cohere(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::ChatGptOAuth => build_chatgpt_oauth(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::ClaudeCodeOAuth => build_claude_code_oauth(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
        ProviderType::Antigravity => build_antigravity(
            resolved.provider(),
            resolved.model(),
            api_url,
            env_vars,
            credentials,
        ),
    }
}

// =============================================================================
// OpenAI (Chat and Responses)
// =============================================================================

fn build_openai_chat(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "openai")]
    {
        let api_key = require_env_value(
            credentials,
            provider_key,
            OPENAI_COMPATIBLE_PROVIDER,
            env_vars,
            "a credential",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::OpenAIChatModel::new(model_name, api_key);
        if let Some(api_url) = api_url {
            model = model.with_base_url(api_url);
        }
        Ok(ResolvedSerdesModel::new(
            OPENAI_COMPATIBLE_PROVIDER,
            model_name,
            model,
        ))
    }
    #[cfg(not(feature = "openai"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("openai", OPENAI_COMPATIBLE_PROVIDER))
    }
}

fn build_openai_responses(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "openai")]
    {
        let api_key = require_env_value(
            credentials,
            provider_key,
            OPENAI_COMPATIBLE_PROVIDER,
            env_vars,
            "a credential",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::OpenAIResponsesModel::new(model_name, api_key);
        if let Some(api_url) = api_url {
            model = model.with_base_url(api_url);
        }
        Ok(ResolvedSerdesModel::new(
            OPENAI_COMPATIBLE_PROVIDER,
            model_name,
            model,
        ))
    }
    #[cfg(not(feature = "openai"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("openai", OPENAI_COMPATIBLE_PROVIDER))
    }
}

// =============================================================================
// Anthropic
// =============================================================================

fn build_anthropic(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "anthropic")]
    {
        let api_key = require_env_value(
            credentials,
            provider_key,
            "anthropic",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::AnthropicModel::new(model_name, api_key);
        if let Some(api_url) = api_url {
            model = model.with_base_url(api_url);
        }
        Ok(ResolvedSerdesModel::new("anthropic", model_name, model))
    }
    #[cfg(not(feature = "anthropic"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("anthropic", "anthropic"))
    }
}

// =============================================================================
// Google
// =============================================================================

fn build_google(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(any(feature = "google", feature = "gemini"))]
    {
        let api_key = require_env_value(
            credentials,
            provider_key,
            "google",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::google::GoogleModel::new(model_name, api_key);
        if let Some(api_url) = api_url {
            model = model.with_base_url(api_url);
        }
        Ok(ResolvedSerdesModel::new("google", model_name, model))
    }
    #[cfg(not(any(feature = "google", feature = "gemini")))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(ModelError::configuration(
            "provider `google` is not enabled in llm-coding-tools-serdesai; rebuild with `--features google` or `--features gemini`"
        ))
    }
}

// =============================================================================
// Groq (fixed endpoint - no URL override allowed)
// =============================================================================

/// Compares two URLs for equality, ignoring trailing slashes.
///
/// URLs often include or omit trailing slashes inconsistently, but represent the same
/// endpoint. This normalizes both sides before comparison.
#[inline]
fn urls_equal_ignoring_slash(lhs: &str, rhs: &str) -> bool {
    lhs.trim_end_matches('/') == rhs.trim_end_matches('/')
}

/// Validates that a provider with a fixed endpoint isn't configured with a different URL.
///
/// Some providers (Groq, Cohere, OpenRouter) have hardcoded base URLs in their model
/// implementations and don't support custom endpoints. If the catalog specifies a URL
/// that differs from the expected one, this returns a configuration error.
fn validate_fixed_endpoint(
    provider_key: &str,
    provider_name: &str,
    api_url: Option<&str>,
    expected_url: &str,
) -> Result<(), ModelError> {
    if let Some(api_url) = api_url
        && !urls_equal_ignoring_slash(api_url, expected_url)
    {
        return Err(ModelError::configuration(format!(
            "provider `{provider_key}` mapped to serdes `{provider_name}` uses catalog api url `{api_url}`, but the SerdesAI `{provider_name}` model does not support overriding its built-in endpoint `{expected_url}`"
        )));
    }
    Ok(())
}

fn build_groq(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "groq")]
    {
        validate_fixed_endpoint(
            provider_key,
            "groq",
            api_url,
            serdes_ai_models::GroqModel::BASE_URL,
        )?;
        let api_key = require_env_value(
            credentials,
            provider_key,
            "groq",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        Ok(ResolvedSerdesModel::new(
            "groq",
            model_name,
            serdes_ai_models::GroqModel::new(model_name, api_key),
        ))
    }
    #[cfg(not(feature = "groq"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("groq", "groq"))
    }
}

// =============================================================================
// Mistral
// =============================================================================

fn build_mistral(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "mistral")]
    {
        let api_key = require_env_value(
            credentials,
            provider_key,
            "mistral",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::MistralModel::new(model_name, api_key);
        if let Some(api_url) = api_url {
            model = model.with_base_url(api_url);
        }
        Ok(ResolvedSerdesModel::new("mistral", model_name, model))
    }
    #[cfg(not(feature = "mistral"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("mistral", "mistral"))
    }
}

// =============================================================================
// Ollama
// =============================================================================

fn build_ollama(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "ollama")]
    {
        let _ = (provider_key, env_vars, credentials);
        let mut model = serdes_ai_models::OllamaModel::new(model_name);
        if let Some(api_url) = api_url {
            model = model.with_base_url(api_url);
        }
        Ok(ResolvedSerdesModel::new("ollama", model_name, model))
    }
    #[cfg(not(feature = "ollama"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars, credentials);
        Err(feature_disabled_error("ollama", "ollama"))
    }
}

// =============================================================================
// Bedrock
// =============================================================================

/// Build a Bedrock model.
///
/// Bedrock resolves the standard AWS credential names through [`CredentialLookup`] and passes
/// them into the SerdesAI model constructor. Region remains optional and falls back to the model
/// default when neither `AWS_REGION` nor `AWS_DEFAULT_REGION` is provided.
fn build_bedrock(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "bedrock")]
    {
        let _ = (api_url, env_vars);
        let access_key_id = require_named_value(
            credentials,
            provider_key,
            "bedrock",
            AWS_ACCESS_KEY_ID_ENV_VAR,
            "an AWS access key ID",
        )?;
        let secret_access_key = require_named_value(
            credentials,
            provider_key,
            "bedrock",
            AWS_SECRET_ACCESS_KEY_ENV_VAR,
            "an AWS secret access key",
        )?;
        let mut aws_credentials =
            serdes_ai_models::bedrock::AwsCredentials::new(access_key_id, secret_access_key);
        if let Some(session_token) = credentials.resolve(AWS_SESSION_TOKEN_ENV_VAR) {
            aws_credentials = aws_credentials.with_session_token(session_token);
        }

        let mut model =
            serdes_ai_models::BedrockModel::with_credentials(model_name, aws_credentials);
        if let Some(region) = first_resolved_name(
            credentials,
            &[AWS_REGION_ENV_VAR, AWS_DEFAULT_REGION_ENV_VAR],
        ) {
            model = model.with_region(region);
        }

        Ok(ResolvedSerdesModel::new("bedrock", model_name, model))
    }
    #[cfg(not(feature = "bedrock"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars, credentials);
        Err(feature_disabled_error("bedrock", "bedrock"))
    }
}

// =============================================================================
// Azure OpenAI
// =============================================================================

/// Checks if an environment variable name represents an Azure resource name.
///
/// Azure OpenAI can be identified by either a full endpoint URL or just the resource
/// name (e.g., "my-resource" becomes `https://my-resource.openai.azure.com`).
/// This identifies catalog env vars that contain resource names rather than full URLs.
#[inline]
fn is_azure_resource_name_env_var(env_var: &str) -> bool {
    env_var.ends_with("_RESOURCE_NAME")
}

/// Normalizes an Azure endpoint URL by removing common redundant path suffixes.
///
/// Users may copy endpoints from the Azure portal that include `/openai` or `/openai/v1`
/// suffixes, but the Azure SDK constructs the full path internally as
/// `{endpoint}/openai/deployments/{deployment}`. This function strips those suffixes
/// to prevent double paths like `/openai/openai/deployments/...`.
fn normalize_azure_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_suffix("/openai/v1") {
        stripped.to_owned()
    } else if let Some(stripped) = trimmed.strip_suffix("/openai") {
        stripped.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// Constructs a full Azure endpoint URL from a resource name.
///
/// If the input is already a full URL (starts with http:// or https://), it's normalized.
/// Otherwise, the resource name is converted to the standard Azure format:
/// `https://{resource_name}.openai.azure.com`
fn azure_endpoint_from_resource(resource_name: &str) -> String {
    let trimmed = resource_name.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return normalize_azure_endpoint(trimmed);
    }

    let mut endpoint = String::with_capacity(trimmed.len() + 27);
    endpoint.push_str("https://");
    endpoint.push_str(trimmed);
    endpoint.push_str(".openai.azure.com");
    endpoint
}

/// Resolves the Azure endpoint from catalog configuration or environment variables.
///
/// Priority:
/// 1. Explicit `api_url` from catalog (normalized)
/// 2. `*_RESOURCE_NAME` environment variable (converted to full URL)
///
/// Returns an error if neither is available.
fn resolve_azure_endpoint(
    credentials: &impl CredentialLookup,
    provider_key: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
) -> Result<String, ModelError> {
    if let Some(api_url) = api_url {
        return Ok(normalize_azure_endpoint(api_url));
    }

    if let Some(resource_name) =
        first_matching_env_value(credentials, env_vars, is_azure_resource_name_env_var)
    {
        return Ok(azure_endpoint_from_resource(&resource_name));
    }

    Err(ModelError::configuration(format!(
        "provider `{provider_key}` mapped to serdes `azure` requires an Azure endpoint or resource name; set one of: {}",
        matching_env_names(env_vars, is_azure_resource_name_env_var)
    )))
}

fn build_azure(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "azure")]
    {
        let endpoint = resolve_azure_endpoint(credentials, provider_key, api_url, env_vars)?;
        let api_key = require_env_value(
            credentials,
            provider_key,
            "azure",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        Ok(ResolvedSerdesModel::new(
            "azure",
            model_name,
            serdes_ai_models::AzureOpenAIModel::new(
                model_name,
                endpoint,
                serdes_ai_models::AzureOpenAIModel::DEFAULT_API_VERSION,
                api_key,
            ),
        ))
    }
    #[cfg(not(feature = "azure"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("azure", "azure"))
    }
}

// =============================================================================
// OpenRouter (fixed endpoint - no URL override allowed)
// =============================================================================

fn build_openrouter(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "openrouter")]
    {
        validate_fixed_endpoint(provider_key, "openrouter", api_url, OPENROUTER_BASE_URL)?;
        let api_key = require_env_value(
            credentials,
            provider_key,
            "openrouter",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        Ok(ResolvedSerdesModel::new(
            "openrouter",
            model_name,
            serdes_ai_models::OpenRouterModel::new(model_name, api_key),
        ))
    }
    #[cfg(not(feature = "openrouter"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("openrouter", "openrouter"))
    }
}

// =============================================================================
// HuggingFace
// =============================================================================

fn build_huggingface(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "huggingface")]
    {
        let token = require_env_value(
            credentials,
            provider_key,
            "huggingface",
            env_vars,
            "a token",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::HuggingFaceModel::new(model_name, token);
        if let Some(api_url) = api_url {
            model = model.with_endpoint(api_url);
        }
        Ok(ResolvedSerdesModel::new("huggingface", model_name, model))
    }
    #[cfg(not(feature = "huggingface"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("huggingface", "huggingface"))
    }
}

// =============================================================================
// Cohere (fixed endpoint - no URL override allowed)
// =============================================================================

fn build_cohere(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "cohere")]
    {
        validate_fixed_endpoint(provider_key, "cohere", api_url, COHERE_BASE_URL)?;
        let api_key = require_env_value(
            credentials,
            provider_key,
            "cohere",
            env_vars,
            "an API key",
            is_credential_env_var,
        )?;
        Ok(ResolvedSerdesModel::new(
            "cohere",
            model_name,
            serdes_ai_models::CohereModel::new(model_name, api_key),
        ))
    }
    #[cfg(not(feature = "cohere"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("cohere", "cohere"))
    }
}

// =============================================================================
// ChatGPT OAuth
// =============================================================================

/// Checks if an environment variable name represents a ChatGPT OAuth account ID.
///
/// ChatGPT OAuth supports multiple accounts; this identifies env vars containing
/// an account ID to associate requests with a specific account.
#[inline]
fn is_chatgpt_oauth_account_id_env_var(env_var: &str) -> bool {
    env_var.ends_with("_ACCOUNT_ID")
}

fn build_chatgpt_oauth(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "chatgpt-oauth")]
    {
        let access_token = require_env_value(
            credentials,
            provider_key,
            "chatgpt-oauth",
            env_vars,
            "an access token",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::ChatGptOAuthModel::new(model_name, access_token);
        if let Some(api_url) = api_url {
            model = model.with_config(serdes_ai_models::chatgpt_oauth::ChatGptConfig {
                api_base_url: api_url.to_owned(),
                ..serdes_ai_models::chatgpt_oauth::ChatGptConfig::default()
            });
        }
        if let Some(account_id) =
            first_matching_env_value(credentials, env_vars, is_chatgpt_oauth_account_id_env_var)
        {
            model = model.with_account_id(account_id);
        }
        Ok(ResolvedSerdesModel::new("chatgpt-oauth", model_name, model))
    }
    #[cfg(not(feature = "chatgpt-oauth"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("chatgpt-oauth", "chatgpt-oauth"))
    }
}

// =============================================================================
// Claude Code OAuth
// =============================================================================

fn build_claude_code_oauth(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "claude-code-oauth")]
    {
        let access_token = require_env_value(
            credentials,
            provider_key,
            "claude-code-oauth",
            env_vars,
            "an access token",
            is_credential_env_var,
        )?;
        let mut model = serdes_ai_models::ClaudeCodeOAuthModel::new(model_name, access_token);
        if let Some(api_url) = api_url {
            model = model.with_config(serdes_ai_models::claude_code_oauth::ClaudeCodeConfig {
                api_base_url: api_url.to_owned(),
                ..serdes_ai_models::claude_code_oauth::ClaudeCodeConfig::default()
            });
        }
        Ok(ResolvedSerdesModel::new(
            "claude-code-oauth",
            model_name,
            model,
        ))
    }
    #[cfg(not(feature = "claude-code-oauth"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error(
            "claude-code-oauth",
            "claude-code-oauth",
        ))
    }
}

// =============================================================================
// Antigravity
// =============================================================================

/// Checks if an environment variable name represents an Antigravity project ID.
///
/// Antigravity organizes resources into projects; this identifies env vars containing
/// the project ID for scoping API requests. Falls back to a default if not provided.
#[inline]
fn is_antigravity_project_id_env_var(env_var: &str) -> bool {
    env_var.ends_with("_PROJECT_ID")
}

fn build_antigravity(
    provider_key: &str,
    model_name: &str,
    api_url: Option<&str>,
    env_vars: &[&str],
    credentials: &impl CredentialLookup,
) -> Result<ResolvedSerdesModel, ModelError> {
    #[cfg(feature = "antigravity")]
    {
        let access_token = require_env_value(
            credentials,
            provider_key,
            "antigravity",
            env_vars,
            "an access token",
            is_credential_env_var,
        )?;
        let project_id =
            first_matching_env_value(credentials, env_vars, is_antigravity_project_id_env_var)
                .unwrap_or_else(|| serdes_ai_models::antigravity::DEFAULT_PROJECT_ID.to_owned());
        let mut model =
            serdes_ai_models::AntigravityModel::new(model_name, access_token, project_id);
        if let Some(api_url) = api_url {
            model = model.with_config(serdes_ai_models::antigravity::AntigravityConfig {
                endpoint: api_url.to_owned(),
                ..serdes_ai_models::antigravity::AntigravityConfig::default()
            });
        }
        Ok(ResolvedSerdesModel::new("antigravity", model_name, model))
    }
    #[cfg(not(feature = "antigravity"))]
    {
        let _ = (provider_key, model_name, api_url, env_vars);
        Err(feature_disabled_error("antigravity", "antigravity"))
    }
}

#[cfg(test)]
mod tests;
