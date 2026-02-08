//! Tokio-based async filesystem operations.

use crate::error::ToolResult;
use std::path::Path;

/// Reads a file to string.
pub async fn read_to_string(path: impl AsRef<Path>) -> ToolResult<String> {
    Ok(tokio::fs::read_to_string(path).await?)
}

/// Writes content to a file.
pub async fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> ToolResult<()> {
    Ok(tokio::fs::write(path, contents).await?)
}

/// Creates a directory and all parent directories.
pub async fn create_dir_all(path: impl AsRef<Path>) -> ToolResult<()> {
    Ok(tokio::fs::create_dir_all(path).await?)
}

/// Opens a file for buffered reading.
pub async fn open_buffered(
    path: impl AsRef<Path>,
    capacity: usize,
) -> ToolResult<tokio::io::BufReader<tokio::fs::File>> {
    let file = tokio::fs::File::open(path).await?;
    Ok(tokio::io::BufReader::with_capacity(capacity, file))
}
