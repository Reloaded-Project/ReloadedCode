//! Provider-facing metadata for the `webfetch` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "webfetch";

/// Default timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Maximum timeout in milliseconds.
pub const MAX_TIMEOUT_MS: u64 = 600_000;

/// Maximum response size in mebibytes (for display in prompts).
pub const MAX_RESPONSE_SIZE_MIB: usize = 5;

/// Tool description.
pub const DESCRIPTION: &str =
    "Fetch one URL. HTML is converted to Markdown and JSON is pretty-printed.";

/// Parameter metadata.
pub mod param {
    use super::{ParamMetadata, DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS};
    use const_format::formatcp;

    /// `url` parameter metadata.
    pub const URL: ParamMetadata = ParamMetadata::new("url", "Fully formed URL to fetch.", true);

    /// `timeout_ms` parameter metadata.
    pub const TIMEOUT_MS: ParamMetadata = ParamMetadata::new(
        "timeout_ms",
        formatcp!(
            "Timeout in milliseconds. Default {}, max {}.",
            DEFAULT_TIMEOUT_MS,
            MAX_TIMEOUT_MS
        ),
        false,
    );
}
