//! Catalog loading and synchronization with models.dev.
//!
//! Flow is simple:
//! - Try online sync first using conditional HTTP (`If-None-Match`)
//! - Reuse cache on `304 Not Modified`
//! - Fall back to cached data if the network path fails

mod load_result;

pub use load_result::{CatalogLoadResult, CatalogLoadSource};

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
    /// - `LLM_CODING_TOOLS_MODELS_DEV_CACHE_PATH` environment variable (if set)
    /// - Platform cache directory + `llm-coding-tools/models.dev.catalog.v1.cache`
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
    /// use llm_coding_tools_models_dev::ModelsDevCatalog;
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
    #[maybe_async::maybe_async]
    pub async fn load() -> Result<CatalogLoadResult, CatalogError> {
        todo!("ModelsDevCatalog::load() not yet implemented")
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
    ///
    /// Returns [`CatalogError`] under the same conditions as [`load`](Self::load),
    /// plus:
    /// - The parent directory cannot be created
    /// - The path is not a valid file path
    ///
    /// # Examples
    ///
    /// ```
    /// use llm_coding_tools_models_dev::ModelsDevCatalog;
    /// use std::path::PathBuf;
    ///
    /// # #[cfg(feature = "tokio")]
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let cache_path = PathBuf::from("/tmp/my-cache.cache");
    /// let result = ModelsDevCatalog::load_at(&cache_path).await?;
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
    /// # let cache_path = PathBuf::from("/tmp/my-cache.cache");
    /// # let result = ModelsDevCatalog::load_at(&cache_path)?;
    /// # if let Some(entry) = result.catalog.lookup("openai", "gpt-4") {
    /// #     println!("API URL: {}", entry.0.api_url);
    /// # }
    /// # Ok(())
    /// # }
    /// ```
    #[maybe_async::maybe_async]
    pub async fn load_at(path: impl AsRef<Path>) -> Result<CatalogLoadResult, CatalogError> {
        let _path = path.as_ref();
        todo!("ModelsDevCatalog::load_at() not yet implemented")
    }
}
