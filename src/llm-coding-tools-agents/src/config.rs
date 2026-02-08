//! Agent configuration schema.

use indexmap::IndexMap;
use llm_coding_tools_core::permissions::PermissionAction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Can be selected as primary agent for conversations.
    Primary,
    /// Only available as subagent via Task tool.
    Subagent,
    /// Available in both contexts.
    #[default]
    All,
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
    pub name: Option<String>,
    #[serde(default)]
    pub mode: AgentMode,
    pub description: String,
    #[serde(default)]
    pub model: Option<String>,
    /// Legacy visibility flag accepted for compatibility only.
    ///
    /// Runtime behavior in headless mode ignores this field.
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub permission: IndexMap<String, PermissionRule>,
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
}

/// Agent configuration loaded from a markdown file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Agent name (derived from file path).
    pub name: String,
    /// Execution mode.
    #[serde(default)]
    pub mode: AgentMode,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Optional model override (format: "provider/model-id").
    #[serde(default)]
    pub model: Option<String>,
    /// Legacy visibility flag accepted for compatibility only.
    ///
    /// Runtime behavior in headless mode ignores this field.
    #[serde(default)]
    pub hidden: bool,
    /// Temperature for sampling.
    #[serde(default)]
    pub temperature: Option<f64>,
    /// Top-p for nucleus sampling.
    #[serde(default)]
    pub top_p: Option<f64>,
    /// Tool permissions map.
    #[serde(default)]
    pub permission: IndexMap<String, PermissionRule>,
    /// Arbitrary extra options.
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,
    /// Prompt body (markdown content after frontmatter, preserved exactly).
    #[serde(skip)]
    pub prompt: String,
}

impl AgentConfig {
    /// Creates an [`AgentConfig`] from raw frontmatter and derived values.
    pub(crate) fn from_raw(name: String, raw: RawFrontmatter, prompt: String) -> Self {
        Self {
            name: raw.name.unwrap_or(name),
            mode: raw.mode,
            description: raw.description,
            model: raw.model,
            hidden: raw.hidden,
            temperature: raw.temperature,
            top_p: raw.top_p,
            permission: raw.permission,
            options: raw.options,
            prompt,
        }
    }
}
