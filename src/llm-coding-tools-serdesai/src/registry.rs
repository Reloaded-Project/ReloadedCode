//! SerdesAI agent registry with precomputed tool context and system prompts.

use async_trait::async_trait;
use llm_coding_tools_agents::{AgentCatalog, AgentConfig, AgentMode, Ruleset};
use llm_coding_tools_core::SystemPromptBuilder;
use serde_json::{Map, Value};
use serdes_ai::agent::ModelConfig;
use serdes_ai::{Agent, AgentBuilder, ModelSettings};
use std::collections::HashMap;
use std::sync::Arc;

/// Default model + sampling settings for serdesAI agents.
#[derive(Debug, Clone)]
pub struct AgentDefaults {
    /// Default model ID (e.g., "provider/model-id").
    pub model: String,
    /// Default API key override (if any).
    pub api_key: Option<String>,
    /// Default base URL override (if any).
    pub base_url: Option<String>,
    /// Default temperature override (if any).
    pub temperature: Option<f64>,
    /// Default top-p override (if any).
    pub top_p: Option<f64>,
    /// Default additional model params merged into per-agent options.
    pub options: HashMap<String, Value>,
}

/// Errors returned when building a serdesAI agent registry.
#[derive(Debug)]
pub enum AgentRegistryBuildError {
    /// No model was provided by defaults or agent config.
    MissingModel {
        /// The name of the agent missing a model.
        agent: String,
    },
    /// Failed to build the serdesAI agent instance.
    BuildFailed {
        /// The name of the agent that failed to build.
        agent: String,
        /// The error message describing the failure.
        message: String,
    },
}

impl std::fmt::Display for AgentRegistryBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingModel { agent } => write!(f, "missing model for agent '{agent}'"),
            Self::BuildFailed { agent, message } => {
                write!(f, "failed to build agent '{agent}': {message}")
            }
        }
    }
}

impl std::error::Error for AgentRegistryBuildError {}

/// Error returned by registry agent invocations.
#[derive(Debug, Clone)]
pub struct RegistryAgentError {
    /// Human-readable error message.
    message: String,
}

impl RegistryAgentError {
    /// Creates a new error from any displayable message.
    ///
    /// Parameters:
    /// - `message`: error message to store.
    ///
    /// Returns: a new [`RegistryAgentError`].
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RegistryAgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RegistryAgentError {}

/// Minimal prompt interface used by the registry + Task tool.
#[async_trait]
pub trait RegistryAgent<Deps>: Send + Sync {
    /// Executes a user prompt and returns the agent response.
    ///
    /// Parameters:
    /// - `message`: fully constructed user message (includes task context + prompt).
    /// - `deps`: dependencies passed to the agent run.
    ///
    /// Returns: agent output text on success.
    async fn prompt(&self, message: String, deps: Arc<Deps>) -> Result<String, RegistryAgentError>;
}

#[async_trait]
impl<Deps> RegistryAgent<Deps> for Agent<Arc<Deps>, String>
where
    Deps: Send + Sync + 'static,
{
    async fn prompt(&self, message: String, deps: Arc<Deps>) -> Result<String, RegistryAgentError> {
        let result = self
            .run(message, deps)
            .await
            .map_err(|err| RegistryAgentError::new(err.to_string()))?;
        Ok(result.into_output())
    }
}

/// Precomputed serdesAI registry entry for a single agent.
pub struct AgentRegistryEntry<A> {
    /// Source configuration used to build the agent.
    pub config: AgentConfig,
    /// Allowed tool names after permission filtering.
    pub tool_names: Vec<String>,
    /// Prebuilt system prompt (tool context + agent prompt).
    pub system_prompt: String,
    /// Built serdesAI agent implementation.
    pub agent: A,
}

impl<A> AgentRegistryEntry<A> {
    /// Returns true if the agent can be invoked via Task (Subagent or All).
    ///
    /// Returns: `true` when [`AgentMode::Subagent`] or [`AgentMode::All`].
    #[inline]
    pub fn is_invocable(&self) -> bool {
        matches!(self.config.mode, AgentMode::Subagent | AgentMode::All)
    }
}

/// SerdesAI registry mapping agent name to prebuilt entries.
pub struct AgentRegistry<A> {
    entries: HashMap<String, AgentRegistryEntry<A>>,
}

impl<A> AgentRegistry<A> {
    /// Creates a registry from prebuilt entries.
    ///
    /// Parameters:
    /// - `entries`: iterator of `(name, entry)` pairs.
    ///
    /// Returns: a populated [`AgentRegistry`].
    pub fn from_entries(
        entries: impl IntoIterator<Item = (String, AgentRegistryEntry<A>)>,
    ) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Returns the entry for a given agent name.
    ///
    /// Parameters:
    /// - `name`: agent name to lookup.
    ///
    /// Returns: `Some(&AgentRegistryEntry)` if found; otherwise `None`.
    #[inline]
    pub fn get(&self, name: &str) -> Option<&AgentRegistryEntry<A>> {
        self.entries.get(name)
    }

    /// Returns an iterator over all entries.
    ///
    /// Returns: iterator over `(name, entry)` pairs.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&String, &AgentRegistryEntry<A>)> {
        self.entries.iter()
    }
}

/// Builder for constructing a serdesAI registry from configs + tools.
pub struct AgentRegistryBuilder<Deps> {
    defaults: AgentDefaults,
    tools: Vec<crate::tool_catalog::ToolCatalogEntry>,
    _deps: std::marker::PhantomData<Deps>,
}

impl<Deps> AgentRegistryBuilder<Deps>
where
    Deps: Send + Sync + 'static,
{
    /// Creates a new registry builder.
    ///
    /// Parameters:
    /// - `defaults`: default model + sampling settings.
    /// - `tools`: cloneable tool catalog used for filtering and agent construction.
    ///
    /// Returns: a new [`AgentRegistryBuilder`].
    pub fn new(defaults: AgentDefaults, tools: Vec<crate::tool_catalog::ToolCatalogEntry>) -> Self {
        Self {
            defaults,
            tools,
            _deps: std::marker::PhantomData,
        }
    }

    /// Builds a serdesAI registry from the provided agent catalog.
    ///
    /// Parameters:
    /// - `catalog`: config-only agent catalog.
    ///
    /// Returns: a populated [`AgentRegistry`] or [`AgentRegistryBuildError`].
    pub fn build(
        &self,
        catalog: &AgentCatalog,
    ) -> Result<AgentRegistry<Agent<Arc<Deps>, String>>, AgentRegistryBuildError> {
        let mut entries = HashMap::with_capacity(catalog.iter().count());

        for config in catalog.iter() {
            let model = config
                .model
                .clone()
                .filter(|m| !m.is_empty())
                .or_else(|| Some(self.defaults.model.clone()))
                .filter(|m| !m.is_empty())
                .ok_or_else(|| AgentRegistryBuildError::MissingModel {
                    agent: config.name.clone(),
                })?;
            let temperature = config.temperature.or(self.defaults.temperature);
            let top_p = config.top_p.or(self.defaults.top_p);

            let ruleset = Ruleset::from_config(&config.permission);
            let mut allowed_tools = Vec::with_capacity(self.tools.len());
            let mut tool_names = Vec::with_capacity(self.tools.len());
            for tool in &self.tools {
                if ruleset.is_allowed(tool.name(), "*") {
                    allowed_tools.push(tool.clone());
                    tool_names.push(tool.name().to_string());
                }
            }

            let mut pb = SystemPromptBuilder::new();
            if !config.prompt.is_empty() {
                pb = pb.system_prompt(config.prompt.clone());
            }

            let mut model_config = ModelConfig::new(&model);
            if let Some(api_key) = &self.defaults.api_key {
                model_config = model_config.with_api_key(api_key.clone());
            }
            if let Some(base_url) = &self.defaults.base_url {
                model_config = model_config.with_base_url(base_url.clone());
            }
            let mut builder = AgentBuilder::<Arc<Deps>, String>::from_config(model_config)
                .map_err(|err| AgentRegistryBuildError::BuildFailed {
                    agent: config.name.clone(),
                    message: err.to_string(),
                })?;

            let mut settings = ModelSettings::new();
            if let Some(temp) = temperature {
                settings = settings.temperature(temp);
            }
            if let Some(value) = top_p {
                settings = settings.top_p(value);
            }

            let mut params = Map::with_capacity(self.defaults.options.len() + config.options.len());
            for (key, value) in &self.defaults.options {
                params.insert(key.clone(), value.clone());
            }
            for (key, value) in &config.options {
                params.insert(key.clone(), value.clone());
            }
            if !params.is_empty() {
                settings = settings.extra(Value::Object(params));
            }
            if !settings.is_empty() {
                builder = builder.model_settings(settings);
            }

            for tool in allowed_tools {
                builder = tool.register_with_prompt(builder, &mut pb);
            }

            let system_prompt = pb.build();
            let agent = builder.system_prompt(system_prompt.clone()).build();

            entries.insert(
                config.name.clone(),
                AgentRegistryEntry {
                    config: config.clone(), // AgentConfig is small and cheap to clone for error cases
                    tool_names,
                    system_prompt,
                    agent,
                },
            );
        }

        Ok(AgentRegistry { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::sync::Arc;

    #[test]
    fn agent_defaults_with_all_fields() {
        let mut options = HashMap::new();
        options.insert("key1".to_string(), Value::Bool(true));

        let defaults = AgentDefaults {
            model: "test-model".to_string(),
            api_key: Some("test-key".to_string()),
            base_url: Some("https://example.com".to_string()),
            temperature: Some(0.7),
            top_p: Some(0.9),
            options,
        };

        assert_eq!(defaults.model, "test-model");
        assert_eq!(defaults.api_key.as_deref(), Some("test-key"));
        assert_eq!(defaults.base_url.as_deref(), Some("https://example.com"));
        assert_eq!(defaults.temperature, Some(0.7));
        assert_eq!(defaults.top_p, Some(0.9));
        assert_eq!(defaults.options.len(), 1);
    }

    #[test]
    fn agent_registry_entry_is_invocable() {
        let config = AgentConfig {
            name: "test".to_string(),
            mode: AgentMode::Subagent,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        };

        let entry = AgentRegistryEntry {
            config,
            tool_names: vec!["Read".to_string()],
            system_prompt: String::new(),
            agent: Arc::new(()),
        };

        assert!(entry.is_invocable());
    }

    #[test]
    fn agent_registry_entry_not_invocable_for_primary() {
        let config = AgentConfig {
            name: "test".to_string(),
            mode: AgentMode::Primary,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        };

        let entry = AgentRegistryEntry {
            config,
            tool_names: vec!["Read".to_string()],
            system_prompt: String::new(),
            agent: Arc::new(()),
        };

        assert!(!entry.is_invocable());
    }

    #[test]
    fn agent_registry_from_entries_and_get() {
        let config1 = AgentConfig {
            name: "agent1".to_string(),
            mode: AgentMode::Subagent,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        };

        let entry1 = AgentRegistryEntry {
            config: config1,
            tool_names: vec!["Read".to_string()],
            system_prompt: String::new(),
            agent: Arc::new(()),
        };

        let registry = AgentRegistry::from_entries([("agent1".to_string(), entry1)]);
        let retrieved = registry.get("agent1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().tool_names, vec!["Read".to_string()]);
    }

    #[tokio::test]
    async fn registry_agent_error_display() {
        let error = RegistryAgentError::new("test error message");
        assert_eq!(format!("{}", error), "test error message");
    }

    #[test]
    fn agent_registry_build_error_missing_model() {
        let config = AgentConfig {
            name: "no-model".to_string(),
            mode: AgentMode::Subagent,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        };

        let defaults = AgentDefaults {
            model: "".to_string(), // Empty model
            api_key: None,
            base_url: None,
            temperature: None,
            top_p: None,
            options: HashMap::new(),
        };

        let catalog = AgentCatalog::from_entries(vec![config]);
        let builder = AgentRegistryBuilder::<()>::new(defaults, vec![]);

        let result = builder.build(&catalog);
        assert!(matches!(
            result,
            Err(AgentRegistryBuildError::MissingModel { agent })
            if agent == "no-model"
        ));
    }
}
