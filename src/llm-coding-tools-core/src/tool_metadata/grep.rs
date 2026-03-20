//! Provider-facing metadata for the `grep` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "grep";

/// Default maximum matches to return.
pub const DEFAULT_LIMIT: usize = 100;

/// Maximum allowed matches to return.
pub const MAX_LIMIT: usize = 2000;

/// Tool descriptions.
pub mod description {
    /// Absolute-path variant.
    #[must_use]
    pub const fn absolute(line_numbers: bool) -> &'static str {
        if line_numbers {
            "Search file contents with a regex. Returns matching lines with line numbers, sorted newest first by file."
        } else {
            "Search file contents with a regex. Returns matching lines, sorted newest first by file."
        }
    }

    /// Allowed-path variant.
    #[must_use]
    pub const fn allowed(line_numbers: bool) -> &'static str {
        if line_numbers {
            "Search file contents with a regex in allowed directories. Returns matching lines with line numbers, sorted newest first by file."
        } else {
            "Search file contents with a regex in allowed directories. Returns matching lines, sorted newest first by file."
        }
    }
}

/// Parameter metadata.
pub mod param {
    use super::{ParamMetadata, DEFAULT_LIMIT, MAX_LIMIT};
    use const_format::formatcp;

    /// `pattern` parameter metadata.
    pub const PATTERN: ParamMetadata = ParamMetadata::new("pattern", "Regex to search for.", true);

    /// `path` in absolute-path mode.
    pub const PATH_ABSOLUTE: ParamMetadata =
        ParamMetadata::new("path", "Absolute directory path to search.", true);

    /// `path` in allowed-path mode.
    pub const PATH_ALLOWED: ParamMetadata = ParamMetadata::new(
        "path",
        "Directory path relative to an allowed directory, or an absolute path inside one.",
        true,
    );

    /// `include` parameter metadata.
    pub const INCLUDE: ParamMetadata = ParamMetadata::new(
        "include",
        "File glob filter, e.g. \"*.rs\" or \"*.{ts,tsx}\". If omitted, searches all files.",
        false,
    );

    /// `limit` parameter metadata.
    pub const LIMIT: ParamMetadata = ParamMetadata::new(
        "limit",
        formatcp!(
            "Maximum matches to return. Default {}, max {}.",
            DEFAULT_LIMIT,
            MAX_LIMIT
        ),
        false,
    );
}
