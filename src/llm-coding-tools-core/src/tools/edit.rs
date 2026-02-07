//! File editing tool with exact string replacement.

use crate::error::ToolError;
use crate::fs;
use crate::path::PathResolver;
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
    #[error(
        "oldString found {0} times and requires more code context to uniquely identify the intended match"
    )]
    AmbiguousMatch(usize),
}

impl From<std::io::Error> for EditError {
    fn from(e: std::io::Error) -> Self {
        EditError::Tool(ToolError::from(e))
    }
}

/// Performs exact string replacement in a file.
///
/// Returns success message with replacement count.
#[maybe_async::maybe_async]
pub async fn edit_file<R: PathResolver>(
    resolver: &R,
    file_path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<String, EditError> {
    if old_string.is_empty() {
        return Err(EditError::EmptyOldString);
    }
    if old_string == new_string {
        return Err(EditError::IdenticalStrings);
    }

    let path = resolver.resolve(file_path)?;
    let content = fs::read_to_string(&path).await?;

    let count = content.matches(old_string).count();

    if count == 0 {
        return Err(EditError::NotFound);
    }

    if !replace_all && count > 1 {
        return Err(EditError::AmbiguousMatch(count));
    }

    let new_content = if replace_all {
        content.replace(old_string, new_string)
    } else {
        content.replacen(old_string, new_string, 1)
    };

    fs::write(&path, &new_content).await?;

    Ok(format!("Successfully replaced {} occurrence(s)", count))
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

    #[maybe_async::test(feature = "blocking", async(not(feature = "blocking"), tokio::test))]
    async fn single_replacement_succeeds() {
        let file = create_temp_file("hello world");
        let resolver = AbsolutePathResolver;

        let result = edit_file(
            &resolver,
            file.path().to_str().unwrap(),
            "world",
            "rust",
            false,
        )
        .await
        .unwrap();

        assert!(result.contains("1 occurrence"));
        let content = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(content, "hello rust");
    }

    #[maybe_async::test(feature = "blocking", async(not(feature = "blocking"), tokio::test))]
    async fn not_found_returns_error() {
        let file = create_temp_file("hello world");
        let resolver = AbsolutePathResolver;

        let err = edit_file(
            &resolver,
            file.path().to_str().unwrap(),
            "missing",
            "x",
            false,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, EditError::NotFound));
    }
}
