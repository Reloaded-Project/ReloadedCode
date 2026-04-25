//! # Agent Frontmatter Types
//!
//! Rust data types used to parse agent markdown files.
//!
//! ## File Shape
//! - YAML frontmatter between `---` delimiters
//! - Markdown prompt body after frontmatter
//!
//! ## Example Agent File
//! ```markdown
//! ---
//! name: code-reviewer
//! mode: subagent
//! description: Reviews code and flags high-risk issues
//! model: synthetic/hf:moonshotai/Kimi-K2.5
//! temperature: 0.2
//! top_p: 0.9
//! permission:
//!   read: allow
//!   bash: deny
//!   task:
//!     "*": deny
//!     orchestrator-*: allow
//! tool_settings:
//!   read:
//!     line_numbers: false
//!   grep:
//!     line_numbers: false
//! options:
//!   max_tokens: 4096
//! hidden: false
//! ---
//! You are a careful code reviewer.
//! ```
//!
//! ## Behavior Notes
//! - `name` uses frontmatter when present; otherwise loader-provided default.
//! - [`AgentConfig::prompt`] stores LF newlines and trims outer ASCII whitespace.
//! - `permission` supports scalar (`allow`/`deny`) or pattern-map rules.
//! - `hidden` is accepted for compatibility but ignored in headless runtime.
//! - `tool_settings` controls tool behaviour; defaults to line-numbers enabled.

use super::tool_settings::{deserialize_non_null_tool_settings, AgentToolSettings};
use ahash::AHashMap;
use indexmap::IndexMap;
use reloaded_code_core::permissions::PermissionAction;
use serde::{Deserialize, Serialize};

/// Agent execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Available in both contexts.
    #[default]
    All,
    /// Can be selected as primary agent for conversations.
    Primary,
    /// Only available as subagent via Task tool.
    Subagent,
}

/// Permission rule: simple action or pattern-based map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionRule {
    /// Simple allow/deny for all.
    Action(PermissionAction),
    /// Pattern-based rules (e.g., `{"orchestrator-*": "deny", "*": "allow"}`).
    Pattern(IndexMap<String, PermissionAction>),
}

impl Default for PermissionRule {
    fn default() -> Self {
        Self::Action(PermissionAction::default())
    }
}

/// Raw frontmatter data (intermediate deserialization target).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawFrontmatter {
    #[serde(default)]
    pub name: Option<Box<str>>,
    #[serde(default)]
    pub mode: AgentMode,
    pub description: Box<str>,
    #[serde(default)]
    pub model: Option<Box<str>>,
    /// Legacy visibility flag accepted for compatibility only.
    ///
    /// Runtime behaviour in headless mode ignores this field.
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub permission: IndexMap<String, PermissionRule>,
    #[serde(default, deserialize_with = "deserialize_non_null_tool_settings")]
    pub tool_settings: AgentToolSettings,
    #[serde(default)]
    pub options: AHashMap<String, serde_json::Value>,
}

/// Agent configuration loaded from a markdown file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Resolved agent name.
    ///
    /// This comes from frontmatter `name` when present; otherwise a loader-
    /// provided default (for example, derived from a file path) is used.
    pub name: Box<str>,
    /// Execution mode.
    #[serde(default)]
    pub mode: AgentMode,
    /// Human-readable description.
    #[serde(default)]
    pub description: Box<str>,
    /// Optional model override (format: "provider/model-id").
    ///
    /// Use [`AgentConfig::get_provider_model`] before catalog lookup.
    #[serde(default)]
    pub model: Option<Box<str>>,
    /// Legacy visibility flag accepted for compatibility only.
    ///
    /// Runtime behaviour in headless mode ignores this field.
    #[serde(default)]
    pub hidden: bool,
    /// Temperature for sampling.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Top-p for nucleus sampling.
    #[serde(default)]
    pub top_p: Option<f32>,
    /// Tool permissions map.
    #[serde(default)]
    pub permission: IndexMap<String, PermissionRule>,
    /// Arbitrary extra options.
    #[serde(default)]
    pub options: AHashMap<String, serde_json::Value>,
    /// Tool settings controlling tool behaviour.
    #[serde(default)]
    pub tool_settings: AgentToolSettings,
    /// Prompt body after frontmatter parsing.
    ///
    /// The parser stores this with LF line endings and trims surrounding ASCII
    /// whitespace.
    #[serde(skip)]
    pub prompt: Box<str>,
}

impl AgentConfig {
    /// Returns the provider and model identifier from [`AgentConfig::model`].
    ///
    /// Delegates to [`parse_model_parts`] for parsing.
    ///
    /// ## Expected Format
    /// `"provider/model-id"` (e.g., `"openai/gpt-4"`, `"synthetic/hf:moonshotai/Kimi-K2.5"`).
    ///
    /// ## Returns
    /// - `Some(("provider", "model-id"))` on valid input
    /// - `None` if [`AgentConfig::model`] is unset or malformed (missing `/` or empty segments)
    #[inline]
    pub fn get_provider_model(&self) -> Option<(&str, &str)> {
        self.model.as_deref().and_then(parse_model_parts)
    }

    /// Creates an [`AgentConfig`] from raw frontmatter and parsed prompt body.
    pub(crate) fn from_raw(
        default_name: impl Into<Box<str>>,
        raw: RawFrontmatter,
        prompt: impl Into<Box<str>>,
    ) -> Self {
        let name = match raw.name {
            Some(s) => s,
            None => default_name.into(),
        };
        Self {
            name,
            mode: raw.mode,
            description: raw.description,
            model: raw.model,
            hidden: raw.hidden,
            temperature: raw.temperature,
            top_p: raw.top_p,
            permission: raw.permission,
            options: raw.options,
            tool_settings: raw.tool_settings,
            prompt: prompt.into(),
        }
    }
}

/// Parses a model identifier string into `(provider, model)` parts.
///
/// ## Expected Format
/// `"provider/model-id"` (e.g., `"openai/gpt-4"`, `"synthetic/hf:moonshotai/Kimi-K2.5"`).
///
/// ## Returns
/// - `Some(("provider", "model-id"))` on valid input
/// - `None` if the value lacks a `/` separator or has empty segments
#[inline]
pub fn parse_model_parts(value: &str) -> Option<(&str, &str)> {
    let (provider, model) = value.split_once('/')?;
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider, model))
}

#[cfg(test)]
mod tests {
    use super::{AgentConfig, AgentMode, AgentToolSettings};
    use ahash::AHashMap;
    use indexmap::IndexMap;

    fn config_with_model(model: Option<&str>) -> AgentConfig {
        AgentConfig {
            name: "example".into(),
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

    #[test]
    fn get_provider_model_returns_provider_and_model() {
        let config = config_with_model(Some("synthetic/hf:moonshotai/Kimi-K2.5"));

        assert_eq!(
            config.get_provider_model(),
            Some(("synthetic", "hf:moonshotai/Kimi-K2.5"))
        );
    }

    #[test]
    fn get_provider_model_rejects_missing_separator() {
        let config = config_with_model(Some("synthetic-only"));

        assert_eq!(config.get_provider_model(), None);
    }

    #[test]
    fn get_provider_model_handles_absent_model() {
        let config = config_with_model(None);

        assert_eq!(config.get_provider_model(), None);
    }
}
