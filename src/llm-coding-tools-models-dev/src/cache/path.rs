//! Cross-platform cache path resolution.

use crate::{error::CatalogResult, CatalogError};
use std::path::PathBuf;

/// Environment variable name for overriding the default cache path.
pub const CACHE_PATH_ENV_VAR: &str = "LLM_CODING_TOOLS_MODELS_DEV_CACHE_PATH";

const CACHE_SUBDIR: &str = "llm-coding-tools";
const CACHE_FILENAME: &str = "models.dev.catalog.v1.cache";

/// Returns the shared cache path for the models.dev catalog.
///
/// This function determines the appropriate cache location using the following
/// precedence:
///
/// 1. `LLM_CODING_TOOLS_MODELS_DEV_CACHE_PATH` environment variable (if set)
/// 2. Platform cache directory + `llm-coding-tools/models.dev.catalog.v1.cache`
///
/// # Platform Cache Locations
///
/// - **Linux**: `~/.cache/llm-coding-tools/models.dev.catalog.v1.cache`
/// - **macOS**: `~/Library/Caches/llm-coding-tools/models.dev.catalog.v1.cache`
/// - **Windows**: `%LOCALAPPDATA%\llm-coding-tools\models.dev.catalog.v1.cache`
///
/// # Returns
///
/// The full path to the cache file.
///
/// # Errors
/// - Returns [`CatalogError::Configuration`] when `LLM_CODING_TOOLS_MODELS_DEV_CACHE_PATH`
///   is set but empty.
/// - Returns [`CatalogError::CachePathNotFound`] when the environment variable is not set
///   and the platform cache directory cannot be determined.
///
/// # Examples
///
/// ```
/// use llm_coding_tools_models_dev::shared_cache_path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let path = shared_cache_path()?;
/// println!("Cache location: {}", path.display());
/// # Ok(())
/// # }
/// ```
pub fn shared_cache_path() -> CatalogResult<PathBuf> {
    // 1. Check env var first
    if let Some(os_str) = std::env::var_os(CACHE_PATH_ENV_VAR) {
        if os_str.is_empty() {
            return Err(CatalogError::Configuration(format!(
                "{} is set but empty",
                CACHE_PATH_ENV_VAR
            )));
        }
        return Ok(PathBuf::from(&os_str));
    }

    // 2. Fall back to dirs::cache_dir()
    let cache_dir = dirs::cache_dir().ok_or(CatalogError::CachePathNotFound)?;

    Ok(cache_dir.join(CACHE_SUBDIR).join(CACHE_FILENAME))
}
