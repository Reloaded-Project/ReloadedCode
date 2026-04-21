//! File editing tool with exact string replacement.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::path::PathResolver;
use crate::tool_metadata::edit as edit_meta;
use serde::Deserialize;
use serde_json::Value;
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
    ///
    /// # Errors
    /// - Returns [`ToolError::Json`] when the JSON payload cannot be deserialized
    ///   into an [`EditRequest`] (e.g., missing required `file_path`, `old_string`,
    ///   or `new_string` fields, or invalid field types).
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

/// Runtime settings for edit requests.
///
/// Reserved for future use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditSettings {}

impl EditSettings {
    /// Creates default edit settings.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

/// Performs exact string replacement in a file.
///
/// Returns success message with replacement count.
///
/// # Errors
/// - Returns [`EditError::EmptyOldString`] when `request.old_string` is empty.
/// - Returns [`EditError::IdenticalStrings`] when `old_string` and `new_string`
///   are identical.
/// - Returns [`EditError::Tool`] wrapping [`ToolError::InvalidPath`] when path
///   resolution fails.
/// - Returns [`EditError::Tool`] wrapping [`ToolError::Io`] when reading the file fails.
/// - Returns [`EditError::NotFound`] when `old_string` is not found in the file content.
/// - Returns [`EditError::AmbiguousMatch`] when `replace_all=false` and multiple
///   occurrences of `old_string` are found (requires more context to identify unique match).
/// - Returns [`EditError::Tool`] wrapping [`ToolError::Io`] when writing the modified
///   content back to the file fails.
#[maybe_async::maybe_async]
pub async fn edit_file<R: PathResolver>(
    resolver: &R,
    request: EditRequest,
    _settings: &EditSettings,
) -> Result<String, EditError> {
    if request.old_string.is_empty() {
        return Err(EditError::EmptyOldString);
    }
    if request.old_string == request.new_string {
        return Err(EditError::IdenticalStrings);
    }

    let path = resolver.resolve(&request.file_path)?;
    let content = fs::read_to_string(&path).await?;

    let (new_content, replacement_count) = if request.replace_all {
        // Single-pass: build the result string while counting every match.
        let needle_len = request.old_string.len();
        let mut result = String::with_capacity(content.len());
        let mut last_end = 0;
        let mut count: usize = 0;
        for (idx, _) in content.match_indices(&request.old_string) {
            result.push_str(&content[last_end..idx]);
            result.push_str(&request.new_string);
            last_end = idx + needle_len;
            count += 1;
        }
        if count == 0 {
            return Err(EditError::NotFound);
        }
        result.push_str(&content[last_end..]);
        (result, count)
    } else {
        // Fast path for single replacement: find the first match, then check for a second to detect ambiguity.
        let needle_len = request.old_string.len();
        let Some(first_idx) = content.find(&request.old_string) else {
            return Err(EditError::NotFound);
        };
        // E.g. "aa" in "aaa" — two overlapping occurrences starting at 0 and 1.
        if content[first_idx + 1..].contains(&request.old_string) {
            return Err(EditError::AmbiguousMatch);
        }
        // Build the edited string directly from slices to avoid rescanning.
        let tail_start = first_idx + needle_len;
        let mut replaced =
            String::with_capacity(content.len() - needle_len + request.new_string.len());
        replaced.push_str(&content[..first_idx]);
        replaced.push_str(&request.new_string);
        replaced.push_str(&content[tail_start..]);
        (replaced, 1)
    };

    fs::write(&path, &new_content).await?;

    let mut msg = String::with_capacity(38);
    let _ = core::fmt::write(
        &mut msg,
        core::format_args!("Successfully replaced {} occurrence(s)", replacement_count),
    );
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::AbsolutePathResolver;
    use std::io::Write;
    use tempfile::NamedTempFile;

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
    async fn overlapping_match_is_ambiguous() {
        let file = create_temp_file("aaa");
        let resolver = AbsolutePathResolver;

        let err = edit_file(
            &resolver,
            EditRequest {
                file_path: file.path().to_str().unwrap().to_string(),
                old_string: "aa".to_string(),
                new_string: "x".to_string(),
                replace_all: false,
            },
            &EditSettings::new(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, EditError::AmbiguousMatch),
            "expected AmbiguousMatch for overlapping occurrences of 'aa' in 'aaa', got {:?}",
            err
        );
    }
}
