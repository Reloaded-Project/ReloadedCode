//! Model-facing tool metadata shared across integrations.
//!
//! This module contains provider-facing tool names, descriptions, and parameter
//! metadata used when building tool/function schemas for APIs such as OpenAI or
//! Anthropic. This is separate from [`crate::context`], which contains longer
//! system-prompt guidance.

pub mod bash;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod read;
pub mod task;
pub mod todo_read;
pub mod todo_write;
pub mod webfetch;
pub mod write;

/// Shared parameter metadata for provider-facing tool schemas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamMetadata {
    /// JSON parameter name.
    pub name: &'static str,
    /// Provider-facing parameter description.
    pub description: &'static str,
    /// Whether the parameter is required in the schema.
    pub required: bool,
}

impl ParamMetadata {
    /// Creates parameter metadata.
    #[must_use]
    pub const fn new(name: &'static str, description: &'static str, required: bool) -> Self {
        Self {
            name,
            description,
            required,
        }
    }
}

/// Backward-compatible flat description exports.
pub mod descriptions {
    pub use super::bash::DESCRIPTION as BASH;
    pub use super::edit::description::ABSOLUTE as EDIT_ABSOLUTE;
    pub use super::edit::description::ALLOWED as EDIT_ALLOWED;
    pub use super::glob::description::ABSOLUTE as GLOB_ABSOLUTE;
    pub use super::glob::description::ALLOWED as GLOB_ALLOWED;
    pub use super::grep::description::absolute as grep_absolute;
    pub use super::grep::description::allowed as grep_allowed;
    pub use super::read::description::absolute as read_absolute;
    pub use super::read::description::allowed as read_allowed;
    pub use super::task::DESCRIPTION_PREFIX as TASK_PREFIX;
    pub use super::todo_read::DESCRIPTION as TODO_READ;
    pub use super::todo_write::DESCRIPTION as TODO_WRITE;
    pub use super::webfetch::DESCRIPTION as WEBFETCH;
    pub use super::write::description::ABSOLUTE as WRITE_ABSOLUTE;
    pub use super::write::description::ALLOWED as WRITE_ALLOWED;
}

#[cfg(test)]
mod tests {
    use super::{
        bash, edit, glob, grep, read, task, todo_read, todo_write, webfetch, write, ParamMetadata,
    };

    #[test]
    fn param_metadata_new_stores_fields() {
        let param = ParamMetadata::new("path", "Absolute file path.", true);
        assert_eq!(param.name, "path");
        assert_eq!(param.description, "Absolute file path.");
        assert!(param.required);
    }

    #[test]
    fn tool_name_constants_are_not_empty() {
        assert!(!bash::NAME.is_empty());
        assert!(!edit::NAME.is_empty());
        assert!(!glob::NAME.is_empty());
        assert!(!grep::NAME.is_empty());
        assert!(!read::NAME.is_empty());
        assert!(!task::NAME.is_empty());
        assert!(!todo_read::NAME.is_empty());
        assert!(!todo_write::NAME.is_empty());
        assert!(!webfetch::NAME.is_empty());
        assert!(!write::NAME.is_empty());
    }

    #[test]
    fn compatibility_descriptions_remain_available() {
        assert!(!super::descriptions::BASH.is_empty());
        assert!(!super::descriptions::TODO_READ.is_empty());
        assert!(!super::descriptions::TODO_WRITE.is_empty());
        assert!(!super::descriptions::WEBFETCH.is_empty());
        assert!(!super::descriptions::WRITE_ABSOLUTE.is_empty());
        assert!(!super::descriptions::WRITE_ALLOWED.is_empty());
        assert!(!super::descriptions::EDIT_ABSOLUTE.is_empty());
        assert!(!super::descriptions::EDIT_ALLOWED.is_empty());
        assert!(!super::descriptions::GLOB_ABSOLUTE.is_empty());
        assert!(!super::descriptions::GLOB_ALLOWED.is_empty());
        assert!(!super::descriptions::TASK_PREFIX.is_empty());
        assert_ne!(
            super::descriptions::read_absolute(true),
            super::descriptions::read_absolute(false)
        );
        assert_ne!(
            super::descriptions::grep_allowed(true),
            super::descriptions::grep_allowed(false)
        );
    }
}
