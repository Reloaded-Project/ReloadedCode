//! File editing tool with exact string replacement.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::path::PathResolver;
use crate::permissions::Ruleset;
use crate::permissions_ext::OptionRulesetExt;
use crate::tool_metadata::edit as edit_meta;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

/// Errors specific to edit tools.
#[derive(Debug, Error)]
pub enum EditError {
    /// I/O or path validation error.
    #[error(transparent)]
    Tool(#[from] ToolError),
    /// The old_string parameter was empty.
    #[error("old_string must not be empty")]
    EmptyOldString,
    /// The old_string and new_string are identical.
    #[error("old_string and new_string must be different")]
    IdenticalStrings,
    /// The old_string was not found in the file.
    #[error("old_string not found in file content")]
    NotFound,
    /// Multiple matches found when replace_all is false.
    ///
    /// This variant intentionally does not include an exact count so single-replace
    /// mode can stop searching as soon as it finds a second match.
    #[error(
        "old_string found multiple times and requires more code context to uniquely identify the intended match"
    )]
    AmbiguousMatch,
}

/// Serde-friendly edit request owned by the core crate.
#[derive(Debug, Deserialize)]
pub struct EditRequest {
    pub file_path: String,
    pub old_string: String,
    pub new_string: String,
    #[serde(default = "edit_meta::default_replace_all")]
    pub replace_all: bool,
}

impl EditRequest {
    /// Parses a raw JSON tool payload into an edit request.
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

impl From<std::io::Error> for EditError {
    fn from(e: std::io::Error) -> Self {
        EditError::Tool(ToolError::from(e))
    }
}

impl From<EditError> for ToolError {
    fn from(err: EditError) -> Self {
        match err {
            EditError::NotFound => {
                ToolError::validation_for("old_string", "old_string not found in file content")
            }
            EditError::AmbiguousMatch => ToolError::validation_for(
                "old_string",
                "old_string found multiple times and requires more code context to uniquely identify the intended match",
            ),
            EditError::EmptyOldString => {
                ToolError::validation_for("old_string", "old_string must not be empty")
            }
            EditError::IdenticalStrings => {
                ToolError::validation_for("old_string", "old_string and new_string must be different")
            }
            EditError::Tool(tool_err) => tool_err,
        }
    }
}

/// Runtime settings that control permission filtering for edit requests.
///
/// Wraps an optional [`Ruleset`] that gates which paths an [`edit_file`]
/// operation may target.
///
/// [`Ruleset`]: crate::permissions::Ruleset
/// [`edit_file`]: edit_file
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditSettings {
    permission: Option<Arc<Ruleset>>,
}

impl EditSettings {
    /// Creates default edit settings with no extra permission filtering.
    ///
    /// # Returns
    /// - An [`EditSettings`] with permission set to `None`.
    #[must_use]
    pub fn new() -> Self {
        Self { permission: None }
    }

    /// Attaches an optional permission ruleset to edit operations.
    ///
    /// # Arguments
    /// - `permission` - An optional [`Arc<Ruleset>`] controlling which paths
    ///   may be edited. Pass `None` to disable permission filtering.
    ///
    /// # Returns
    /// - The modified [`EditSettings`] with the permission attached.
    ///
    /// [`Arc<Ruleset>`]: std::sync::Arc
    #[must_use]
    pub fn with_permission(mut self, permission: Option<Arc<Ruleset>>) -> Self {
        self.permission = permission;
        self
    }

    /// Returns the permission ruleset applied to edit operations, if any.
    ///
    /// # Returns
    /// - `Some(&`[`Ruleset`]`)` when a permission filter is configured.
    /// - `None` when no permission filtering is applied.
    ///
    /// [`Ruleset`]: crate::permissions::Ruleset
    #[must_use]
    pub fn permission(&self) -> Option<&Ruleset> {
        self.permission.as_deref()
    }
}

/// Performs exact string replacement in a file.
///
/// Returns success message with replacement count.
#[maybe_async::maybe_async]
pub async fn edit_file<R: PathResolver>(
    resolver: &R,
    request: EditRequest,
    settings: &EditSettings,
) -> Result<String, EditError> {
    if request.old_string.is_empty() {
        return Err(EditError::EmptyOldString);
    }
    if request.old_string == request.new_string {
        return Err(EditError::IdenticalStrings);
    }

    let path = resolver.resolve(&request.file_path)?;
    let subject = path.to_string_lossy();
    settings
        .permission()
        .check(edit_meta::NAME, subject.as_ref())?;
    let content = fs::read_to_string(&path).await?;

    let (new_content, replacement_count) = if request.replace_all {
        // replace_all reports the exact number of replacements, so this path
        // counts every match.
        let count = content.matches(&request.old_string).count();
        if count == 0 {
            return Err(EditError::NotFound);
        }

        (
            content.replace(&request.old_string, &request.new_string),
            count,
        )
    } else {
        // Fast path for single replacement: advance a single non-overlapping
        // matcher until the second match (if any), then stop.
        let mut matches = content.match_indices(&request.old_string);
        let Some((first_idx, _)) = matches.next() else {
            return Err(EditError::NotFound);
        };
        if matches.next().is_some() {
            return Err(EditError::AmbiguousMatch);
        }

        let tail_start = first_idx + request.old_string.len();

        // Build the edited string directly from slices to avoid rescanning.
        let mut replaced = String::with_capacity(
            content.len() - request.old_string.len() + request.new_string.len(),
        );
        replaced.push_str(&content[..first_idx]);
        replaced.push_str(&request.new_string);
        replaced.push_str(&content[tail_start..]);
        (replaced, 1)
    };

    fs::write(&path, &new_content).await?;

    Ok(format!(
        "Successfully replaced {} occurrence(s)",
        replacement_count
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::AbsolutePathResolver;
    use crate::permissions::{ExpandError, PermissionAction, Rule};
    use std::io::Write;
    use tempfile::NamedTempFile;

    type TestResult = Result<(), ExpandError>;

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn single_replacement_succeeds() {
        let file = create_temp_file("hello world");
        let resolver = AbsolutePathResolver;

        let result = edit_file(
            &resolver,
            EditRequest {
                file_path: file.path().to_str().unwrap().to_string(),
                old_string: "world".to_string(),
                new_string: "rust".to_string(),
                replace_all: false,
            },
            &EditSettings::new(),
        )
        .await
        .unwrap();

        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(content, "hello rust");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn not_found_returns_error() {
        let file = create_temp_file("hello world");
        let resolver = AbsolutePathResolver;

        let err = edit_file(
            &resolver,
            EditRequest {
                file_path: file.path().to_str().unwrap().to_string(),
                old_string: "missing".to_string(),
                new_string: "x".to_string(),
                replace_all: false,
            },
            &EditSettings::new(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, EditError::NotFound));
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn edit_rejects_denied_path() -> TestResult {
        let file = create_temp_file("hello world");
        let resolver = AbsolutePathResolver;

        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("edit", "*", PermissionAction::Allow)?);
        ruleset.push(Rule::new(
            "edit",
            file.path().to_string_lossy().into_owned(),
            PermissionAction::Deny,
        )?);

        let err = edit_file(
            &resolver,
            EditRequest {
                file_path: file.path().to_string_lossy().into_owned(),
                old_string: "world".to_string(),
                new_string: "rust".to_string(),
                replace_all: false,
            },
            &EditSettings::new().with_permission(Some(Arc::new(ruleset))),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            err,
            EditError::Tool(ToolError::PermissionDenied { tool: "edit", .. })
        ));
        Ok(())
    }
}
