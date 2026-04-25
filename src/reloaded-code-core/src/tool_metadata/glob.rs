//! Provider-facing metadata for the `glob` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "glob";

/// Maximum number of results returned.
pub const MAX_RESULTS: usize = 1000;

/// Tool descriptions.
pub mod description {
    /// Absolute-path variant.
    pub const ABSOLUTE: &str =
        "Find files by glob pattern. Respects .gitignore and sorts newest first.";

    /// Allowed-path variant.
    pub const ALLOWED: &str =
        "Find files by glob pattern in allowed directories. Respects .gitignore and sorts newest first.";
}

/// Parameter metadata.
pub mod param {
    use super::ParamMetadata;

    /// `pattern` parameter metadata.
    pub const PATTERN: ParamMetadata = ParamMetadata::new(
        "pattern",
        "Glob pattern, e.g. \"**/*.rs\" or \"src/**/*.ts\".",
        true,
    );

    /// `path` in absolute-path mode.
    pub const PATH_ABSOLUTE: ParamMetadata =
        ParamMetadata::new("path", "Absolute directory path to search.", true);

    /// `path` in allowed-path mode.
    pub const PATH_ALLOWED: ParamMetadata = ParamMetadata::new(
        "path",
        "Directory path relative to an allowed directory, or an absolute path inside one.",
        true,
    );
}
