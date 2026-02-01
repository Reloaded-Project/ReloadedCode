//! Rig-native agent registry built from [`AgentCatalog`].

use async_trait::async_trait;
use llm_coding_tools_agents::{AgentCatalog, AgentConfig, AgentMode, Ruleset};
use llm_coding_tools_core::SystemPromptBuilder;
use serde_json::Value;
use std::collections::HashMap;

/// Default model + sampling settings for rig agent construction.
#[derive(Debug, Clone)]
pub struct AgentDefaults {
    /// Default model ID (e.g., "provider/model-id").
    pub model: String,
    /// Default temperature override (if any).
    pub temperature: Option<f64>,
    /// Default top-p override (if any).
    pub top_p: Option<f64>,
    /// Default additional model params merged into per-agent options.
    pub options: HashMap<String, Value>,
}

/// Errors returned when building a rig agent registry.
#[derive(Debug)]
pub enum AgentRegistryBuildError {
    /// No model was provided by defaults or agent config.
    MissingModel {
        /// The name of the agent missing a model.
        agent: String,
    },
    /// Failed to build the rig agent instance.
    BuildFailed {
        /// The name of the agent that failed to build.
        agent: String,
        /// The error message describing the failure.
        message: String,
    },
}

/// Error returned by registry agent invocations.
#[derive(Debug, Clone)]
pub struct RegistryAgentError {
    /// Human-readable error message.
    pub message: String,
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

/// Minimal prompt interface used by the registry + Task tool.
#[async_trait]
pub trait RegistryAgent: Send + Sync {
    /// Executes a user prompt and returns the agent response.
    ///
    /// Parameters:
    /// - `message`: fully constructed user message (includes task context + prompt).
    ///
    /// Returns: agent output text on success.
    async fn prompt(&self, message: String) -> Result<String, RegistryAgentError>;
}

#[async_trait]
impl<M> RegistryAgent for rig::agent::Agent<M>
where
    M: rig::completion::CompletionModel + Send + Sync,
{
    async fn prompt(&self, message: String) -> Result<String, RegistryAgentError> {
        rig::completion::Prompt::prompt(self, message)
            .await
            .map_err(|err| RegistryAgentError::new(err.to_string()))
    }
}

/// Precomputed rig registry entry for a single agent.
pub struct AgentRegistryEntry<A> {
    /// Source configuration used to build the agent.
    pub config: AgentConfig,
    /// Allowed tool names after permission filtering.
    pub tool_names: Vec<String>,
    /// Prebuilt system prompt (tool context + agent prompt).
    pub system_prompt: String,
    /// Built rig-native agent implementation.
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

/// Rig-native registry mapping agent name to prebuilt entries.
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

/// Builder for constructing a rig-native registry from configs + tools.
pub struct AgentRegistryBuilder<M, F>
where
    M: rig::completion::CompletionModel,
    F: Fn(&str) -> rig::agent::AgentBuilder<M>,
{
    build_agent: F,
    defaults: AgentDefaults,
    tools: Vec<crate::tool_catalog::ToolCatalogEntry>,
}

impl<M, F> AgentRegistryBuilder<M, F>
where
    M: rig::completion::CompletionModel,
    F: Fn(&str) -> rig::agent::AgentBuilder<M>,
{
    /// Creates a new registry builder.
    ///
    /// Parameters:
    /// - `build_agent`: closure that returns a rig agent builder for a model id.
    /// - `defaults`: default model + sampling settings.
    /// - `tools`: cloneable tool catalog used for filtering and agent construction.
    ///
    /// Returns: a new [`AgentRegistryBuilder`].
    pub fn new(
        build_agent: F,
        defaults: AgentDefaults,
        tools: Vec<crate::tool_catalog::ToolCatalogEntry>,
    ) -> Self {
        Self {
            build_agent,
            defaults,
            tools,
        }
    }

    /// Builds a rig-native registry from the provided agent catalog.
    ///
    /// Parameters:
    /// - `catalog`: config-only agent catalog.
    ///
    /// Returns: a populated [`AgentRegistry`] or [`AgentRegistryBuildError`].
    pub fn build(
        &self,
        catalog: &AgentCatalog,
    ) -> Result<AgentRegistry<rig::agent::Agent<M>>, AgentRegistryBuildError> {
        let mut entries = HashMap::with_capacity(catalog.iter().count());

        for config in catalog.iter() {
            // 1) Resolve model + sampling defaults (config overrides defaults).
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

            // 2) Build ruleset and filter tools by permission before construction.
            let ruleset = Ruleset::from_config(&config.permission);
            let mut allowed_tools = Vec::with_capacity(self.tools.len());
            let mut tool_names = Vec::with_capacity(self.tools.len());
            for tool in &self.tools {
                if ruleset.is_allowed(tool.name(), "*") {
                    allowed_tools.push(tool.clone());
                    tool_names.push(tool.name().to_string());
                }
            }

            // 3) Precompute system prompt using tracked tool contexts.
            let mut pb = SystemPromptBuilder::new();
            if !config.prompt.is_empty() {
                pb = pb.system_prompt(config.prompt.clone());
            }

            // 4) Build agent with filtered tools + precomputed prompt.
            let mut base_builder = Some((self.build_agent)(&model));
            if let Some(temp) = temperature {
                if let Some(builder) = base_builder.take() {
                    base_builder = Some(builder.temperature(temp));
                }
            }

            let mut params = serde_json::Map::with_capacity(
                self.defaults.options.len() + config.options.len() + 1,
            );
            for (k, v) in &self.defaults.options {
                params.insert(k.clone(), v.clone());
            }
            for (k, v) in &config.options {
                params.insert(k.clone(), v.clone());
            }
            if let Some(p) = top_p {
                params.insert("top_p".to_string(), Value::from(p));
            }
            if !params.is_empty() {
                if let Some(builder) = base_builder.take() {
                    base_builder = Some(builder.additional_params(Value::Object(params)));
                }
            }

            let mut agent_builder: Option<rig::agent::AgentBuilderSimple<M>> = None;
            for tool in allowed_tools {
                agent_builder = Some(match agent_builder.take() {
                    None => {
                        let builder = base_builder.take().ok_or_else(|| {
                            AgentRegistryBuildError::BuildFailed {
                                agent: config.name.clone(),
                                message: "base builder unavailable before first tool".to_string(),
                            }
                        })?;
                        tool.register_on_with_prompt(builder, &mut pb)
                    }
                    Some(b) => tool.register_on_simple_with_prompt(b, &mut pb),
                });
            }

            let system_prompt = pb.build();
            let agent = match agent_builder {
                Some(b) => b.preamble(&system_prompt).build(),
                None => {
                    let builder =
                        base_builder.ok_or_else(|| AgentRegistryBuildError::BuildFailed {
                            agent: config.name.clone(),
                            message: "base builder unavailable when no tools registered"
                                .to_string(),
                        })?;
                    builder.preamble(&system_prompt).build()
                }
            };

            entries.insert(
                config.name.clone(),
                AgentRegistryEntry {
                    config: config.clone(),
                    tool_names,
                    system_prompt,
                    agent,
                },
            );
        }

        Ok(AgentRegistry { entries })
    }
}
