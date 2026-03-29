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
//!     max_timeout_ms: 600000    # default: 600000ms (tool_metadata::bash::MAX_TIMEOUT_MS)
//!   webfetch:
//!     timeout_ms: 30000         # default: 30000ms (tool_metadata::webfetch::DEFAULT_TIMEOUT_MS)
//!     max_timeout_ms: 600000    # default: 600000ms (tool_metadata::webfetch::MAX_TIMEOUT_MS)
//!     max_response_size_mib: 5  # default: 5 MiB (tool_metadata::webfetch::MAX_RESPONSE_SIZE_MIB)
//! ---
//! ```

use llm_coding_tools_core::tool_metadata::{bash, glob, grep, read, webfetch};
use llm_coding_tools_core::util::{MIN_LIMIT, MIN_LINE_LENGTH, MIN_TIMEOUT_MS};
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
    #[serde(
        default = "read_default_limit",
        deserialize_with = "deserialize_min_limit"
    )]
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
    #[serde(
        default = "grep_default_limit",
        deserialize_with = "deserialize_min_limit"
    )]
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
    #[serde(
        default = "glob_default_limit",
        deserialize_with = "deserialize_min_limit"
    )]
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
    #[serde(
        default = "bash_default_timeout_ms",
        deserialize_with = "deserialize_min_timeout_ms"
    )]
    pub timeout_ms: u32,
    /// Maximum timeout allowed for LLM requests (default: 600000, min: timeout_ms).
    #[serde(default = "bash_default_max_timeout_ms")]
    pub max_timeout_ms: u32,
}

impl Default for BashToolSettings {
    fn default() -> Self {
        Self {
            timeout_ms: bash_default_timeout_ms(),
            max_timeout_ms: bash_default_max_timeout_ms(),
        }
    }
}

/// Settings for the webfetch tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebFetchToolSettings {
    /// Timeout in milliseconds (default: 30000, min: 1000).
    #[serde(
        default = "webfetch_default_timeout_ms",
        deserialize_with = "deserialize_min_timeout_ms"
    )]
    pub timeout_ms: u32,
    /// Maximum timeout allowed for LLM requests (default: 600000, min: timeout_ms).
    #[serde(default = "webfetch_default_max_timeout_ms")]
    pub max_timeout_ms: u32,
    /// Maximum response size in MiB (default: 5, min: 1).
    #[serde(
        default = "webfetch_default_max_response_size_mib",
        deserialize_with = "deserialize_min_limit"
    )]
    pub max_response_size_mib: usize,
}

impl Default for WebFetchToolSettings {
    fn default() -> Self {
        Self {
            timeout_ms: webfetch_default_timeout_ms(),
            max_timeout_ms: webfetch_default_max_timeout_ms(),
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
const fn bash_default_timeout_ms() -> u32 {
    bash::DEFAULT_TIMEOUT_MS
}

#[inline]
const fn bash_default_max_timeout_ms() -> u32 {
    bash::MAX_TIMEOUT_MS
}

#[inline]
const fn webfetch_default_timeout_ms() -> u32 {
    webfetch::DEFAULT_TIMEOUT_MS
}

#[inline]
const fn webfetch_default_max_timeout_ms() -> u32 {
    webfetch::MAX_TIMEOUT_MS
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

fn deserialize_min_limit<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value < MIN_LIMIT {
        return Err(serde::de::Error::custom(format!(
            "value must be >= {}",
            MIN_LIMIT
        )));
    }
    Ok(value)
}

fn deserialize_min_timeout_ms<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value < MIN_TIMEOUT_MS {
        return Err(serde::de::Error::custom(format!(
            "value must be >= {}",
            MIN_TIMEOUT_MS
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
    let value = Option::<AgentToolSettings>::deserialize(deserializer)?
        .ok_or_else(|| serde::de::Error::custom("tool_settings cannot be null"))?;

    value.validate().map_err(serde::de::Error::custom)?;
    Ok(value)
}

impl AgentToolSettings {
    /// Validates cross-field constraints (e.g., timeout_ms <= max_timeout_ms).
    #[inline]
    fn validate(&self) -> Result<(), String> {
        validate_timeout_pair("bash", self.bash.timeout_ms, self.bash.max_timeout_ms)?;
        validate_timeout_pair(
            "webfetch",
            self.webfetch.timeout_ms,
            self.webfetch.max_timeout_ms,
        )?;
        Ok(())
    }
}

#[inline]
fn validate_timeout_pair(tool: &str, timeout_ms: u32, max_timeout_ms: u32) -> Result<(), String> {
    if max_timeout_ms < timeout_ms {
        return Err(format!(
            "{tool}.max_timeout_ms ({max_timeout_ms}) must be >= {tool}.timeout_ms ({timeout_ms})"
        ));
    }
    Ok(())
}
