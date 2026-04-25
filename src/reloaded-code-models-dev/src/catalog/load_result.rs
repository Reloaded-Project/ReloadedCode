//! Result types for catalog load operations.

use reloaded_code_core::models::ModelCatalog;

/// Result of a successful catalog load operation.
///
/// This struct provides both the loaded catalog and metadata about
/// how the catalog was obtained (fresh download, cached, etc.).
pub struct CatalogLoadResult {
    /// The loaded model catalog ready for lookups.
    pub catalog: ModelCatalog,

    /// Information about how the catalog was loaded.
    pub source: CatalogLoadSource,
}

/// Indicates how the catalog was loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogLoadSource {
    /// The catalog was downloaded fresh (HTTP 200 OK) and the cache was updated.
    Downloaded,

    /// The cache was up to date (HTTP 304 Not Modified) and loaded from disk.
    NotModifiedCache,

    /// A network failure occurred, but a valid cached copy was available
    /// and loaded as a fallback.
    FallbackCache,
}

impl CatalogLoadSource {
    /// Returns true if the catalog was loaded from the network (fresh download).
    #[inline]
    pub fn is_fresh(&self) -> bool {
        matches!(self, CatalogLoadSource::Downloaded)
    }

    /// Returns true if the catalog was loaded from cache (either fresh cache or fallback).
    #[inline]
    pub fn is_cached(&self) -> bool {
        matches!(
            self,
            CatalogLoadSource::NotModifiedCache | CatalogLoadSource::FallbackCache
        )
    }

    /// Returns true if this was a fallback load due to network failure.
    #[inline]
    pub fn is_fallback(&self) -> bool {
        matches!(self, CatalogLoadSource::FallbackCache)
    }
}
