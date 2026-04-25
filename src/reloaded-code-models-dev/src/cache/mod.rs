//! Cache path and container utilities for models.dev catalog data.
//!
//! Responsibilities are split by concern:
//!
//! - `path` resolves the shared cache location.
//! - `format` defines the cache container layout and read/write helpers.
//!
//! Runtime behavior follows crate features:
//! - `tokio` (default): async file I/O APIs.
//! - `blocking`: sync file I/O APIs.
//!
//! The public API currently exposes path resolution only; container helpers are
//! crate-internal until the sync/load flow is wired.

pub(crate) mod format;
mod path;
pub(crate) mod payload;

pub use crate::error::CatalogResult;
pub use path::{shared_cache_path, CACHE_PATH_ENV_VAR};
