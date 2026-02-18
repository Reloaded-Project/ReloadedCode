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

use ahash::AHashMap;
use indexmap::IndexMap;
use llm_coding_tools_core::permissions::PermissionAction;
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
    pub options: AHashMap<String, serde_json::Value>,
}

/// Agent configuration loaded from a markdown file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Resolved agent name.
    ///
    /// This comes from frontmatter `name` when present; otherwise a loader-
    /// provided default (for example, derived from a file path) is used.
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
    pub options: AHashMap<String, serde_json::Value>,
    /// Prompt body after frontmatter parsing.
    ///
    /// The parser stores this with LF line endings and trims surrounding ASCII
    /// whitespace.
    #[serde(skip)]
    pub prompt: String,
}

impl AgentConfig {
    /// Creates an [`AgentConfig`] from raw frontmatter and parsed prompt body.
    pub(crate) fn from_raw(default_name: String, raw: RawFrontmatter, prompt: String) -> Self {
        Self {
            name: raw.name.unwrap_or(default_name),
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
