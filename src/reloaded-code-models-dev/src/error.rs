//! Error types for models.dev catalog operations.

use reloaded_code_core::models::ModelCatalogBuildError;
use thiserror::Error;

/// Errors that can occur during catalog loading and synchronization.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// The platform's cache directory could not be determined.
    #[error("cache directory not found on this platform")]
    CachePathNotFound,

    /// A configuration error occurred (e.g., invalid environment variable).
    #[error("configuration error: {0}")]
    Configuration(String),

    /// An I/O error occurred while reading or writing the cache.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// An HTTP error occurred during the sync request.
    #[error("HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),

    /// A JSON parse error occurred while decoding models.dev API JSON.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// A zstd decompression error occurred.
    #[error("decompression error: {0}")]
    Zstd(String),

    /// A bitcode deserialization error occurred.
    #[error("decode error: {0}")]
    BitcodeDecode(String),

    /// The on-disk cache file is malformed or incompatible.
    #[error("cache format error: {0}")]
    CacheFormat(&'static str),

    /// The catalog failed to build from source rows.
    #[error("catalog build error: {0}")]
    ModelCatalogBuild(#[from] ModelCatalogBuildError),

    /// A spawn_blocking task failed.
    #[cfg(feature = "tokio")]
    #[error("blocking task failed: {0}")]
    JoinHandle(#[from] tokio::task::JoinError),
}

/// Convenience type alias for catalog operations.
pub type CatalogResult<T> = Result<T, CatalogError>;
