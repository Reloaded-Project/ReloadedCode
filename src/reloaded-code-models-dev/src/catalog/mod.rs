//! Catalog loading and synchronization with models.dev.
//!
//! Flow is simple:
//! - Try online sync first using conditional HTTP (`If-None-Match`)
//! - Reuse cache on `304 Not Modified`
//! - Fall back to cached data if the network path fails

mod load_cache;
mod load_result;
mod sync;

#[cfg(test)]
mod test_utils;

pub use load_result::{CatalogLoadResult, CatalogLoadSource};

use crate::cache::shared_cache_path;
use crate::error::CatalogError;
use std::path::Path;

/// Entry point for loading models.dev catalogs.
///
/// This struct provides static methods for loading the catalog either
/// from the default shared cache location or from a custom path.
pub struct ModelsDevCatalog;

impl ModelsDevCatalog {
    /// Loads the catalog from the default shared cache location.
    ///
    /// This is the primary entry point for most use cases. It will:
    /// 1. Check for an existing cache and extract its ETag
    /// 2. Send a conditional GET request with `If-None-Match`
    /// 3. On `200 OK`: download, map the API payload into catalog sources,
    ///    cache it, and return fresh data
    /// 4. On `304 Not Modified`: decode and return cached data
    /// 5. On network failure: fall back to cached data if available
    ///
    /// The cache location is determined by:
    /// - `RELOADED_CODE_MODELS_DEV_CACHE_PATH` environment variable (if set)
    /// - Platform cache directory + `reloaded-code/models.dev.catalog.v1.cache`
    ///
    /// # Returns
    ///
    /// A [`CatalogLoadResult`] containing the loaded catalog and information
    /// about how it was loaded (downloaded fresh, from cache, or fallback).
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] when:
    /// - The cache path cannot be determined and no cache exists
    /// - An HTTP error occurs and no cache is available for fallback
    /// - The cache is corrupted and cannot be decoded
    /// - Catalog construction from mapped catalog sources fails
    ///
    /// # Examples
    ///
    /// ```
    /// use reloaded_code_models_dev::ModelsDevCatalog;
    ///
    /// # #[cfg(feature = "tokio")]
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let result = ModelsDevCatalog::load().await?;
    ///
    /// // Use the catalog
    /// if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
    ///     println!("API URL: {}", entry.0.api_url);
    /// }
    /// # Ok(())
    /// # }
    ///
    /// # #[cfg(feature = "blocking")]
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let result = ModelsDevCatalog::load()?;
    /// // Use the catalog
    /// # if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
    /// #     println!("API URL: {}", entry.0.api_url);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// - Returns [`CatalogError::CachePathNotFound`] when the environment variable is not set
    ///   and the platform cache directory cannot be determined.
    /// - Returns [`CatalogError::Configuration`] when the environment variable is set but empty.
    /// - Returns [`CatalogError::Io`] when cache file I/O fails without a usable fallback.
    /// - Returns [`CatalogError::Reqwest`] when the HTTP request fails and no valid cache
    ///   is available for fallback.
    /// - Returns [`CatalogError::CacheFormat`] when the cache file is truncated or corrupted.
    /// - Returns [`CatalogError::Zstd`] when zstd decompression fails.
    /// - Returns [`CatalogError::BitcodeDecode`] when the cached payload cannot be decoded.
    /// - Returns [`CatalogError::ModelCatalogBuild`] when catalog reconstruction fails.
    #[maybe_async::maybe_async]
    pub async fn load() -> Result<CatalogLoadResult, CatalogError> {
        let path = shared_cache_path()?;
        Self::load_at(path).await
    }

    /// Loads the catalog from a specific cache file path.
    ///
    /// This method provides the same behavior as [`load`](Self::load), but
    /// allows specifying a custom cache file path. This is useful for:
    /// - Testing with temporary cache files
    /// - Custom deployment scenarios
    /// - Isolated cache locations
    ///
    /// # Parameters
    ///
    /// * `path` - The path to the cache file. Parent directories will be
    ///   created if they don't exist.
    ///
    /// # Returns
    ///
    /// A [`CatalogLoadResult`] containing the loaded catalog and source
    /// information.
    ///
    /// # Errors
    /// - Returns [`CatalogError::Io`] when cache file I/O fails without a usable fallback.
    /// - Returns [`CatalogError::Reqwest`] when the HTTP request fails and no valid cache
    ///   is available for fallback.
    /// - Returns [`CatalogError::CacheFormat`] when the cache file is truncated or corrupted.
    /// - Returns [`CatalogError::Zstd`] when zstd decompression fails.
    /// - Returns [`CatalogError::BitcodeDecode`] when the cached payload cannot be decoded.
    /// - Returns [`CatalogError::ModelCatalogBuild`] when catalog reconstruction fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use reloaded_code_models_dev::ModelsDevCatalog;
    ///
    /// # #[cfg(feature = "tokio")]
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let result = ModelsDevCatalog::load_at("/tmp/models.dev.cache").await?;
    ///
    /// // Use the catalog
    /// if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
    ///     println!("API URL: {}", entry.0.api_url);
    /// }
    /// # Ok(())
    /// # }
    ///
    /// # #[cfg(feature = "blocking")]
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let result = ModelsDevCatalog::load_at("/tmp/models.dev.cache")?;
    /// # if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
    /// #     println!("API URL: {}", entry.0.api_url);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn load_at(path: impl AsRef<Path>) -> Result<CatalogLoadResult, CatalogError> {
        sync::load_catalog_at_path(path.as_ref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CACHE_PATH_ENV_VAR;
    use reloaded_code_core::models::ProviderType;
    use tempfile::TempDir;

    /// Guard that restores the shared cache path env var on drop.
    struct CachePathGuard {
        previous: Option<std::ffi::OsString>,
    }

    impl CachePathGuard {
        fn new(value: &std::ffi::OsStr) -> Self {
            let previous = std::env::var_os(CACHE_PATH_ENV_VAR);
            unsafe {
                std::env::set_var(CACHE_PATH_ENV_VAR, value);
            }
            Self { previous }
        }
    }

    impl Drop for CachePathGuard {
        fn drop(&mut self) {
            super::sync::set_test_models_dev_api_url(None);
            unsafe {
                match self.previous.take() {
                    Some(value) => std::env::set_var(CACHE_PATH_ENV_VAR, value),
                    None => std::env::remove_var(CACHE_PATH_ENV_VAR),
                }
            }
        }
    }

    use super::test_utils::{sample_api_json, start_mock_server, MockResponse};

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    #[serial_test::serial]
    async fn facade_load_uses_shared_cache_path() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("facade-test.cache");
        let _guard = CachePathGuard::new(cache_path.as_os_str());

        let body = String::from_utf8_lossy(sample_api_json()).to_string();
        let (_handle, url) = start_mock_server(MockResponse::Ok {
            etag: "\"facade-test-etag\"",
            body,
        });
        super::sync::set_test_models_dev_api_url(Some(url));

        let result = ModelsDevCatalog::load().await.expect("load should succeed");

        assert_eq!(result.source, CatalogLoadSource::Downloaded);
        let provider = result
            .catalog
            .lookup_provider("openai")
            .expect("openai provider should exist");
        assert_eq!(provider.api_type, ProviderType::OpenAiCompletions);

        // Verify cache was written
        assert!(
            cache_path.exists(),
            "cache file should exist at shared path"
        );
    }
}
