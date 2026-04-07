//! File writing operation.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::path::PathResolver;
use crate::permissions::Ruleset;
use crate::permissions_ext::OptionRulesetExt;
use crate::tool_metadata::write as write_meta;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

/// Serde-friendly write request owned by the core crate.
#[derive(Debug, Deserialize)]
pub struct WriteRequest {
    pub file_path: String,
    pub content: String,
}

impl WriteRequest {
    /// Parses a raw JSON tool payload into a write request.
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings that control permission filtering for write requests.
///
/// Wraps an optional [`Ruleset`] that gates which paths a [`write_file`]
/// operation may target.
///
/// [`Ruleset`]: crate::permissions::Ruleset
/// [`write_file`]: write_file
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteSettings {
    permission: Option<Arc<Ruleset>>,
}

impl WriteSettings {
    /// Creates default write settings with no extra permission filtering.
    ///
    /// # Returns
    /// - A [`WriteSettings`] with permission set to `None`.
    #[must_use]
    pub fn new() -> Self {
        Self { permission: None }
    }

    /// Attaches an optional permission ruleset to write operations.
    ///
    /// # Arguments
    /// - `permission` - An optional [`Arc<Ruleset>`] controlling which paths
    ///   may be written to. Pass `None` to disable permission filtering.
    ///
    /// # Returns
    /// - The modified [`WriteSettings`] with the permission attached.
    ///
    /// [`Arc<Ruleset>`]: std::sync::Arc
    #[must_use]
    pub fn with_permission(mut self, permission: Option<Arc<Ruleset>>) -> Self {
        self.permission = permission;
        self
    }

    /// Returns the permission ruleset applied to write operations, if any.
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

/// Writes content to a file, creating parent directories if needed.
///
/// Overwrites existing files. Returns a success message with byte count.
#[maybe_async::maybe_async]
pub async fn write_file<R: PathResolver>(
    resolver: &R,
    request: WriteRequest,
    settings: &WriteSettings,
) -> ToolResult<String> {
    let path = resolver.resolve(&request.file_path)?;
    let subject = path.to_string_lossy();
    settings
        .permission()
        .check(write_meta::NAME, subject.as_ref())?;

    // Create parent directories if they don't exist
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }

    let bytes = request.content.as_bytes();
    fs::write(&path, bytes).await?;

    Ok(format!(
        "Successfully wrote {} bytes to {}",
        bytes.len(),
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::AbsolutePathResolver;
    use crate::permissions::{ExpandError, PermissionAction, Rule};
    use tempfile::TempDir;

    type TestResult = Result<(), ExpandError>;

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_creates_new_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("new_file.txt");
        let resolver = AbsolutePathResolver;

        let result = write_file(
            &resolver,
            WriteRequest {
                file_path: file_path.to_str().unwrap().to_string(),
                content: "hello world".to_string(),
            },
            &WriteSettings::new(),
        )
        .await
        .unwrap();

        assert!(result.contains("11 bytes"));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_creates_parent_directories() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("a/b/c/deep.txt");
        let resolver = AbsolutePathResolver;

        write_file(
            &resolver,
            WriteRequest {
                file_path: file_path.to_str().unwrap().to_string(),
                content: "nested".to_string(),
            },
            &WriteSettings::new(),
        )
        .await
        .unwrap();

        assert!(file_path.exists());
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_rejects_denied_path() -> TestResult {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("denied.txt");
        let resolver = AbsolutePathResolver;

        let mut ruleset = Ruleset::new();
        ruleset.push(Rule::new("write", "*", PermissionAction::Allow)?);
        ruleset.push(Rule::new(
            "write",
            file_path.to_string_lossy().into_owned(),
            PermissionAction::Deny,
        )?);

        let err = write_file(
            &resolver,
            WriteRequest {
                file_path: file_path.to_string_lossy().into_owned(),
                content: "blocked".to_string(),
            },
            &WriteSettings::new().with_permission(Some(Arc::new(ruleset))),
        )
        .await
        .unwrap_err();

        assert!(matches!(
            err,
            ToolError::PermissionDenied { tool: "write", .. }
        ));
        Ok(())
    }
}
