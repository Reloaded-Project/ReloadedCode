//! Provider-facing metadata for the `read` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "read";

/// Default 1-based line offset.
pub const DEFAULT_OFFSET: usize = 1;

/// Default maximum lines to return.
pub const DEFAULT_LIMIT: usize = 2000;

/// Maximum characters per output line before truncation.
pub const MAX_LINE_LENGTH: usize = 2000;

/// Format prefix for line-numbered output (e.g. `L42: ...`).
pub const LINE_PREFIX_FORMAT: &str = "L{}: ";

/// Display hint for the line-number prefix in prompts.
pub const LINE_PREFIX_DISPLAY: &str = "L{n}: ";

/// Serde-friendly default offset helper.
#[must_use]
pub const fn default_offset() -> usize {
    DEFAULT_OFFSET
}

/// Serde-friendly default line limit helper.
#[must_use]
pub const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// Tool descriptions.
pub mod description {
    /// Absolute-path variant.
    #[must_use]
    pub const fn absolute(line_numbers: bool) -> &'static str {
        if line_numbers {
            "Read a file and return line-numbered text."
        } else {
            "Read a file and return raw text."
        }
    }

    /// Allowed-path variant.
    #[must_use]
    pub const fn allowed(line_numbers: bool) -> &'static str {
        if line_numbers {
            "Read a file from allowed directories and return line-numbered text."
        } else {
            "Read a file from allowed directories and return raw text."
        }
    }
}

/// Parameter metadata.
pub mod param {
    use super::{ParamMetadata, DEFAULT_LIMIT};
    use const_format::formatcp;

    /// `file_path` in absolute-path mode.
    pub const FILE_PATH_ABSOLUTE: ParamMetadata =
        ParamMetadata::new("file_path", "Absolute file path.", true);

    /// `file_path` in allowed-path mode.
    pub const FILE_PATH_ALLOWED: ParamMetadata = ParamMetadata::new(
        "file_path",
        "File path relative to an allowed directory, or an absolute path inside one.",
        true,
    );

    /// `offset` parameter metadata.
    pub const OFFSET: ParamMetadata =
        ParamMetadata::new("offset", "1-based start line. Default 1.", false);

    /// `limit` parameter metadata.
    pub const LIMIT: ParamMetadata = ParamMetadata::new(
        "limit",
        formatcp!("Maximum lines to return. Default {}.", DEFAULT_LIMIT),
        false,
    );
}
