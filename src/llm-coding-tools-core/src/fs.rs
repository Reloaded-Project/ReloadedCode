//! Filesystem abstraction layer.
//!
//! Provides unified APIs that work with both sync and async runtimes.
//! When the `blocking` feature is disabled (default), async operations use tokio.
//! When `blocking` is enabled, all operations are synchronous.

use crate::error::ToolResult;
use std::path::Path;

// ============================================================================
// Async implementations (blocking feature disabled)
// ============================================================================

/// Reads a file to string.
#[cfg(not(feature = "blocking"))]
pub async fn read_to_string(path: impl AsRef<Path>) -> ToolResult<String> {
    Ok(tokio::fs::read_to_string(path).await?)
}

/// Writes content to a file.
#[cfg(not(feature = "blocking"))]
pub async fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> ToolResult<()> {
    Ok(tokio::fs::write(path, contents).await?)
}

/// Creates a directory and all parent directories.
#[cfg(not(feature = "blocking"))]
pub async fn create_dir_all(path: impl AsRef<Path>) -> ToolResult<()> {
    Ok(tokio::fs::create_dir_all(path).await?)
}

/// Opens a file for buffered reading.
#[cfg(not(feature = "blocking"))]
pub async fn open_buffered(
    path: impl AsRef<Path>,
    capacity: usize,
) -> ToolResult<tokio::io::BufReader<tokio::fs::File>> {
    let file = tokio::fs::File::open(path).await?;
    Ok(tokio::io::BufReader::with_capacity(capacity, file))
}

// ============================================================================
// Sync implementations (blocking feature enabled)
// ============================================================================

/// Reads a file to string.
#[cfg(feature = "blocking")]
pub fn read_to_string(path: impl AsRef<Path>) -> ToolResult<String> {
    Ok(std::fs::read_to_string(path)?)
}

/// Writes content to a file.
#[cfg(feature = "blocking")]
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> ToolResult<()> {
    Ok(std::fs::write(path, contents)?)
}

/// Creates a directory and all parent directories.
#[cfg(feature = "blocking")]
pub fn create_dir_all(path: impl AsRef<Path>) -> ToolResult<()> {
    Ok(std::fs::create_dir_all(path)?)
}

/// Opens a file for buffered reading.
#[cfg(feature = "blocking")]
pub fn open_buffered(
    path: impl AsRef<Path>,
    capacity: usize,
) -> ToolResult<std::io::BufReader<std::fs::File>> {
    let file = std::fs::File::open(path)?;
    Ok(std::io::BufReader::with_capacity(capacity, file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[maybe_async::test(feature = "blocking", async(not(feature = "blocking"), tokio::test))]
    async fn read_to_string_works() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        let content = read_to_string(file.path()).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[maybe_async::test(feature = "blocking", async(not(feature = "blocking"), tokio::test))]
    async fn write_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        write(&path, b"hello").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }
}
