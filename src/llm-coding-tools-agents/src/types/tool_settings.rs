//! # Tool Settings Types
//!
//! Per-agent configuration for tool behaviour.
//!
//! All settings use defaults from the tool metadata constants when not specified.
//!
//! ## Frontmatter Schema Example
//! ```yaml
//! ---
//! name: example-agent
//! tool_settings:
//!   read:
//!     line_numbers: true        # default: true
//!     limit: 2000               # default: 2000 (tool_metadata::read::DEFAULT_LIMIT)
//!     max_line_length: 2000     # default: 2000 (tool_metadata::read::MAX_LINE_LENGTH)
//!   grep:
//!     line_numbers: true        # default: true
//!     limit: 100                # default: 100 (tool_metadata::grep::DEFAULT_LIMIT)
//!     max_line_length: 2000     # default: 2000 (tool_metadata::tools::DEFAULT_MAX_LINE_LENGTH)
//!   glob:
//!     limit: 1000               # default: 1000 (tool_metadata::glob::MAX_RESULTS)
//!   bash:
//!     timeout_ms: 120000        # default: 120000ms (tool_metadata::bash::DEFAULT_TIMEOUT_MS)
//!   webfetch:
//!     timeout_ms: 30000         # default: 30000ms (tool_metadata::webfetch::DEFAULT_TIMEOUT_MS)
//!     max_response_size_mib: 5  # default: 5 MiB (tool_metadata::webfetch::MAX_RESPONSE_SIZE_MIB)
//! ---
//! ```

use llm_coding_tools_core::tool_metadata::{bash, glob, grep, read, webfetch};
use llm_coding_tools_core::util::MIN_LINE_LENGTH;
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
    /// Settings for the glob tool.
    #[serde(default)]
    pub glob: GlobToolSettings,
    /// Settings for the bash tool.
    #[serde(default)]
    pub bash: BashToolSettings,
    /// Settings for the webfetch tool.
    #[serde(default)]
    pub webfetch: WebFetchToolSettings,
}

/// Settings for the read tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadToolSettings {
    /// Whether to include line numbers in output (default: true).
    #[serde(default = "default_line_numbers")]
    pub line_numbers: bool,
    /// Maximum lines to return per read (default: 2000, min: 1).
    #[serde(default = "read_default_limit")]
    pub limit: usize,
    /// Maximum characters per line before truncation (default: 2000, min: 4).
    #[serde(
        default = "read_default_max_line_length",
        deserialize_with = "deserialize_read_max_line_length"
    )]
    pub max_line_length: usize,
}

impl Default for ReadToolSettings {
    fn default() -> Self {
        Self {
            line_numbers: true,
            limit: read_default_limit(),
            max_line_length: read_default_max_line_length(),
        }
    }
}

/// Settings for the grep tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrepToolSettings {
    /// Whether to include line numbers in output (default: true).
    #[serde(default = "default_line_numbers")]
    pub line_numbers: bool,
    /// Maximum matches to return (default: 100, min: 1).
    #[serde(default = "grep_default_limit")]
    pub limit: usize,
    /// Maximum characters per line before truncation (default: 2000, min: 4).
    #[serde(
        default = "grep_default_max_line_length",
        deserialize_with = "deserialize_grep_max_line_length"
    )]
    pub max_line_length: usize,
}

impl Default for GrepToolSettings {
    fn default() -> Self {
        Self {
            line_numbers: true,
            limit: grep_default_limit(),
            max_line_length: grep_default_max_line_length(),
        }
    }
}

/// Settings for the glob tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobToolSettings {
    /// Maximum files to return (default: 1000, min: 1).
    #[serde(default = "glob_default_limit")]
    pub limit: usize,
}

impl Default for GlobToolSettings {
    fn default() -> Self {
        Self {
            limit: glob_default_limit(),
        }
    }
}

/// Settings for the bash tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BashToolSettings {
    /// Default timeout in milliseconds (default: 120000, min: 1000).
    #[serde(default = "bash_default_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for BashToolSettings {
    fn default() -> Self {
        Self {
            timeout_ms: bash_default_timeout_ms(),
        }
    }
}

/// Settings for the webfetch tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebFetchToolSettings {
    /// Timeout in milliseconds (default: 30000, min: 1000).
    #[serde(default = "webfetch_default_timeout_ms")]
    pub timeout_ms: u64,
    /// Maximum response size in MiB (default: 5, min: 1).
    #[serde(default = "webfetch_default_max_response_size_mib")]
    pub max_response_size_mib: usize,
}

impl Default for WebFetchToolSettings {
    fn default() -> Self {
        Self {
            timeout_ms: webfetch_default_timeout_ms(),
            max_response_size_mib: webfetch_default_max_response_size_mib(),
        }
    }
}

#[inline]
const fn default_line_numbers() -> bool {
    true
}

#[inline]
const fn read_default_limit() -> usize {
    read::DEFAULT_LIMIT
}

#[inline]
const fn read_default_max_line_length() -> usize {
    read::MAX_LINE_LENGTH
}

#[inline]
const fn grep_default_limit() -> usize {
    grep::DEFAULT_LIMIT
}

#[inline]
const fn grep_default_max_line_length() -> usize {
    // Grep uses the same max line length as read
    read::MAX_LINE_LENGTH
}

#[inline]
const fn glob_default_limit() -> usize {
    glob::MAX_RESULTS
}

#[inline]
const fn bash_default_timeout_ms() -> u64 {
    bash::DEFAULT_TIMEOUT_MS
}

#[inline]
const fn webfetch_default_timeout_ms() -> u64 {
    webfetch::DEFAULT_TIMEOUT_MS
}

#[inline]
const fn webfetch_default_max_response_size_mib() -> usize {
    webfetch::MAX_RESPONSE_SIZE_MIB
}

fn deserialize_read_max_line_length<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_min_max_line_length(deserializer, "read.max_line_length")
}

fn deserialize_grep_max_line_length<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_min_max_line_length(deserializer, "grep.max_line_length")
}

fn deserialize_min_max_line_length<'de, D>(
    deserializer: D,
    field_name: &str,
) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value < MIN_LINE_LENGTH {
        return Err(serde::de::Error::custom(format!(
            "{field_name} must be >= {}",
            MIN_LINE_LENGTH
        )));
    }
    Ok(value)
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
