//! File writing operation.

use crate::error::ToolResult;
use crate::fs;
use crate::path::PathResolver;

/// Writes content to a file, creating parent directories if needed.
///
/// Overwrites existing files. Returns a success message with byte count.
#[maybe_async::maybe_async]
pub async fn write_file<R: PathResolver>(
    resolver: &R,
    file_path: &str,
    content: &str,
) -> ToolResult<String> {
    let path = resolver.resolve(file_path)?;

    // Create parent directories if they don't exist
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).await?;
        }
    }

    let bytes = content.as_bytes();
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
    use tempfile::TempDir;

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_creates_new_file() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("new_file.txt");
        let resolver = AbsolutePathResolver;

        let result = write_file(&resolver, file_path.to_str().unwrap(), "hello world")
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

        write_file(&resolver, file_path.to_str().unwrap(), "nested")
            .await
            .unwrap();

        assert!(file_path.exists());
    }
}
