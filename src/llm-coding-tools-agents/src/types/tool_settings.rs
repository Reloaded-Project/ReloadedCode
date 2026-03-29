//! # Tool Settings Types
//!
//! Per-agent configuration for tool behaviour.
//!
//! ## Frontmatter Schema
//! ```yaml
//! ---
//! name: example-agent
//! tool_settings:
//!   read:
//!     line_numbers: false
//!   grep:
//!     line_numbers: false
//! ---
//! ```

use serde::{Deserialize, Serialize};

/// Per-agent tool settings controlling tool behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AgentToolSettings {
    /// Settings for the read tool.
    #[serde(default)]
    pub read: ReadToolSettings,
    /// Settings for the grep tool.
    #[serde(default)]
    pub grep: GrepToolSettings,
}

/// Settings for the read tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadToolSettings {
    /// Whether to include line numbers in output (default: true).
    #[serde(default = "default_line_numbers")]
    pub line_numbers: bool,
}

impl Default for ReadToolSettings {
    fn default() -> Self {
        Self { line_numbers: true }
    }
}

/// Settings for the grep tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrepToolSettings {
    /// Whether to include line numbers in output (default: true).
    #[serde(default = "default_line_numbers")]
    pub line_numbers: bool,
}

impl Default for GrepToolSettings {
    fn default() -> Self {
        Self { line_numbers: true }
    }
}

#[inline]
const fn default_line_numbers() -> bool {
    true
}

/// Deserializes `tool_settings`, rejecting explicit `null` while allowing
/// absence to default.
pub(crate) fn deserialize_non_null_tool_settings<'de, D>(
    deserializer: D,
) -> Result<AgentToolSettings, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<AgentToolSettings>::deserialize(deserializer)?;
    value.ok_or_else(|| serde::de::Error::custom("tool_settings cannot be null"))
}
