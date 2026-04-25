//! Blocking/sync filesystem operations.

use crate::error::ToolResult;
use std::path::Path;

/// Reads a file to string.
///
/// # Errors
/// - Returns [`ToolError::Io`] when the file cannot be read (e.g., file does not exist,
///   permission denied, or other I/O error).
///
/// [`ToolError::Io`]: crate::error::ToolError::Io
pub fn read_to_string(path: impl AsRef<Path>) -> ToolResult<String> {
    Ok(std::fs::read_to_string(path)?)
}

/// Writes content to a file.
///
/// # Errors
/// - Returns [`ToolError::Io`] when the file cannot be written (e.g., parent directory
///   does not exist, permission denied, or other I/O error).
///
/// [`ToolError::Io`]: crate::error::ToolError::Io
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> ToolResult<()> {
    Ok(std::fs::write(path, contents)?)
}

/// Creates a directory and all parent directories.
///
/// # Errors
/// - Returns [`ToolError::Io`] when the directory cannot be created (e.g., permission
///   denied or other I/O error).
///
/// [`ToolError::Io`]: crate::error::ToolError::Io
pub fn create_dir_all(path: impl AsRef<Path>) -> ToolResult<()> {
    Ok(std::fs::create_dir_all(path)?)
}

/// Opens a file for buffered reading.
///
/// # Errors
/// - Returns [`ToolError::Io`] when the file cannot be opened (e.g., file does not exist,
///   permission denied, or other I/O error).
///
/// [`ToolError::Io`]: crate::error::ToolError::Io
pub fn open_buffered(
    path: impl AsRef<Path>,
    capacity: usize,
) -> ToolResult<std::io::BufReader<std::fs::File>> {
    let file = std::fs::File::open(path)?;
    Ok(std::io::BufReader::with_capacity(capacity, file))
}
