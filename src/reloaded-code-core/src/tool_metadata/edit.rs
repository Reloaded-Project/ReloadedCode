//! Provider-facing metadata for the `edit` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "edit";

/// Default value for `replace_all`.
pub const DEFAULT_REPLACE_ALL: bool = false;

/// Serde-friendly default helper for `replace_all`.
#[must_use]
pub const fn default_replace_all() -> bool {
    DEFAULT_REPLACE_ALL
}

/// Tool descriptions.
pub mod description {
    /// Absolute-path variant.
    pub const ABSOLUTE: &str =
        "Replace exact text in a file. Without replace_all, old_string must match exactly once.";

    /// Allowed-path variant.
    pub const ALLOWED: &str =
        "Replace exact text in a file in allowed directories. Without replace_all, old_string must match exactly once.";
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

    /// `old_string` parameter metadata.
    pub const OLD_STRING: ParamMetadata =
        ParamMetadata::new("old_string", "Exact existing text to replace.", true);

    /// `new_string` parameter metadata.
    pub const NEW_STRING: ParamMetadata =
        ParamMetadata::new("new_string", "Replacement text.", true);

    /// `replace_all` parameter metadata.
    pub const REPLACE_ALL: ParamMetadata = ParamMetadata::new(
        "replace_all",
        "Replace every occurrence. Default false.",
        false,
    );
}
