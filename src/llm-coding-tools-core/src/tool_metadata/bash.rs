//! Provider-facing metadata for the `bash` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "bash";

/// Default timeout in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Maximum timeout in milliseconds.
pub const MAX_TIMEOUT_MS: u64 = 600_000;

/// Tool description.
pub const DESCRIPTION: &str = "Run a shell command in a fresh process.";

/// Parameter metadata.
pub mod param {
    use super::{ParamMetadata, DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS};
    use const_format::formatcp;

    /// `command` parameter metadata.
    pub const COMMAND: ParamMetadata = ParamMetadata::new("command", "Shell command to run.", true);

    /// `workdir` parameter metadata.
    pub const WORKDIR: ParamMetadata = ParamMetadata::new(
        "workdir",
        "Absolute working directory. If omitted, uses the tool's default working directory when configured.",
        false,
    );

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
