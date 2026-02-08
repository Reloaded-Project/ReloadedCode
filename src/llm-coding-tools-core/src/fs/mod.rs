//! Filesystem abstraction layer.
//!
//! Provides unified APIs that work with both sync and async runtimes.
//! When the `blocking` feature is disabled (default), async operations use tokio.
//! When `blocking` is enabled, all operations are synchronous.

#[cfg(all(feature = "tokio", feature = "blocking"))]
compile_error!("Features tokio and blocking are mutually exclusive");

#[cfg(not(any(feature = "tokio", feature = "blocking")))]
compile_error!("Either tokio or blocking feature must be enabled for the fs module");

#[cfg(feature = "tokio")]
mod tokio_impl;
#[cfg(feature = "tokio")]
pub use tokio_impl::*;

#[cfg(feature = "blocking")]
mod blocking_impl;
#[cfg(feature = "blocking")]
pub use blocking_impl::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_to_string_works() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();
        file.flush().unwrap();
        let content = read_to_string(file.path()).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        write(&path, b"hello").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }
}
