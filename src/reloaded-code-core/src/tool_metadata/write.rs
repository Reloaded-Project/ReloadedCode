//! Provider-facing metadata for the `write` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "write";

/// Tool descriptions.
pub mod description {
    /// Absolute-path variant.
    pub const ABSOLUTE: &str =
        "Write a file. Creates parent directories and overwrites existing files.";

    /// Allowed-path variant.
    pub const ALLOWED: &str =
        "Write a file in allowed directories. Creates parent directories and overwrites existing files.";
}

/// Parameter metadata.
pub mod param {
    use super::ParamMetadata;

    /// `file_path` in absolute-path mode.
    pub const FILE_PATH_ABSOLUTE: ParamMetadata =
        ParamMetadata::new("file_path", "Absolute file path.", true);

    /// `file_path` in allowed-path mode.
    pub const FILE_PATH_ALLOWED: ParamMetadata = ParamMetadata::new(
        "file_path",
        "File path relative to an allowed directory, or an absolute path inside one.",
        true,
    );

    /// `content` parameter metadata.
    pub const CONTENT: ParamMetadata =
        ParamMetadata::new("content", "Full file contents to write.", true);
}
