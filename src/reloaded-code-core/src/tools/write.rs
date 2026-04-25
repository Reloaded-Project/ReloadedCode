//! File writing operation.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::path::PathResolver;
use serde::Deserialize;
use serde_json::Value;

/// Serde-friendly write request owned by the core crate.
#[derive(Debug, Deserialize)]
pub struct WriteRequest {
    pub file_path: String,
    pub content: String,
}

impl WriteRequest {
    /// Parses a raw JSON tool payload into a write request.
    ///
    /// # Errors
    /// - Returns [`ToolError::Json`] when the JSON payload cannot be deserialized
    ///   into a [`WriteRequest`] (e.g., missing required `file_path` or `content`
    ///   fields, or invalid field types).
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings for write requests.
///
/// Reserved for future use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteSettings {}

impl WriteSettings {
    /// Creates default write settings.
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }
}

/// Writes content to a file, creating parent directories if needed.
///
/// Overwrites existing files. Returns a success message with byte count.
///
/// # Errors
/// - Returns [`ToolError::InvalidPath`] when `resolver.resolve()` fails to
///   resolve `request.file_path` (e.g., path is not absolute or violates policy).
/// - Returns [`ToolError::Io`] when parent directory creation fails (e.g.,
///   permission denied, read-only filesystem).
/// - Returns [`ToolError::Io`] when writing to the file fails (e.g., disk full,
///   permission denied, I/O error).
#[maybe_async::maybe_async]
pub async fn write_file<R: PathResolver>(
    resolver: &R,
    request: WriteRequest,
    _settings: &WriteSettings,
) -> ToolResult<String> {
    let path = resolver.resolve(&request.file_path)?;

    // Create parent directories if they don't exist
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }

    fs::write(&path, request.content.as_bytes()).await?;

    let len = request.content.len();
    // 32: literal overhead ("Successfully wrote  bytes to "), 20: max usize digits, remainder: path
    let mut out = String::with_capacity(32 + 20 + path.as_os_str().len());
    let _ = core::fmt::write(
        &mut out,
        core::format_args!("Successfully wrote {} bytes to {}", len, path.display()),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::AbsolutePathResolver;
    use tempfile::TempDir;

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
}
