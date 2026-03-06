#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

// Validate feature combinations at compile time.
#[cfg(all(feature = "async", feature = "blocking"))]
compile_error!("Features `async` and `blocking` are mutually exclusive.");

#[cfg(not(any(feature = "async", feature = "blocking")))]
compile_error!("Either an async runtime (e.g., `tokio`) or `blocking` feature must be enabled.");

#[allow(dead_code)] // Wired into catalog build/load slices
mod api;
pub mod cache;
pub mod catalog;
pub mod error;
mod fs;

pub use cache::shared_cache_path;
pub use catalog::{CatalogLoadResult, CatalogLoadSource, ModelsDevCatalog};
pub use error::{CatalogError, CatalogResult};
