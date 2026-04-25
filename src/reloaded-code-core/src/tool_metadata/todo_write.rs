//! Provider-facing metadata for the `todowrite` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "todowrite";

/// Tool description.
pub const DESCRIPTION: &str = "Replace the full todo list.";

/// Parameter metadata.
pub mod param {
    use super::ParamMetadata;

    /// `todos` parameter metadata.
    pub const TODOS: ParamMetadata =
        ParamMetadata::new("todos", "Complete todo list to set.", true);

    /// Todo item `id` field metadata.
    pub const ID: ParamMetadata = ParamMetadata::new("id", "Stable non-empty todo id.", true);

    /// Todo item `content` field metadata.
    pub const CONTENT: ParamMetadata =
        ParamMetadata::new("content", "Short imperative task text.", true);

    /// Todo item `status` field metadata.
    pub const STATUS: ParamMetadata = ParamMetadata::new("status", "Task status.", true);

    /// Todo item `priority` field metadata.
    pub const PRIORITY: ParamMetadata = ParamMetadata::new("priority", "Task priority.", true);
}
