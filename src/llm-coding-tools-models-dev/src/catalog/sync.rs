//! Catalog synchronization against the remote models.dev API.
//!
//! This module owns the online-first load path used by
//! [`ModelsDevCatalog`](crate::catalog::ModelsDevCatalog). It reads any cached
//! container in one shot, sends a conditional request with the cached ETag when
//! available, refreshes the cache on `200 OK`, reuses it on `304 Not Modified`,
//! and falls back to cached data when the request fails.

use crate::api::catalog_sources::cache_payload_from_api_json_bytes;
use crate::cache::format::{read_cache_file, write_cache_file, CacheFileData, CacheWriteInput};
use crate::cache::payload::{catalog_from_cache_payload, encode_cache_payload};
use crate::catalog::load_cache::load_catalog_from_cache_file_data;
use crate::catalog::load_result::{CatalogLoadResult, CatalogLoadSource};
use crate::error::{CatalogError, CatalogResult};
use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::StatusCode;
use std::borrow::Cow;
use std::io::ErrorKind;
use std::path::Path;

/// Default production endpoint for the models.dev catalog snapshot.
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";

/// Timeout for HTTP connections and requests in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 30;

#[cfg(test)]
static TEST_MODELS_DEV_API_URL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(test)]
/// Overrides the remote catalog URL for sync tests.
pub(crate) fn set_test_models_dev_api_url(url: Option<String>) {
    *TEST_MODELS_DEV_API_URL.lock().unwrap() = url;
}

/// Returns the active catalog endpoint, including the test override when set.
fn models_dev_api_url() -> Cow<'static, str> {
    #[cfg(test)]
    if let Some(url) = TEST_MODELS_DEV_API_URL.lock().unwrap().clone() {
        return Cow::Owned(url);
    }

    Cow::Borrowed(MODELS_DEV_API_URL)
}

/// Resolves the result to return after a transient request failure.
///
/// Cached data takes precedence over surfacing the request error so callers can
/// continue with the last known-good catalog when possible.
fn load_after_request_failure(
    request_error: reqwest::Error,
    cache_file: Option<&CacheFileData>,
    cache_error: Option<CatalogError>,
) -> CatalogResult<CatalogLoadResult> {
    if let Some(cache_file) = cache_file {
        return load_catalog_from_cache_file_data(cache_file, CatalogLoadSource::FallbackCache);
    }

    if let Some(cache_error) = cache_error {
        return Err(cache_error);
    }

    Err(CatalogError::Reqwest(request_error))
}

#[inline]
fn is_transient_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[maybe_async::maybe_async]
/// Loads the catalog at `path` using the default models.dev endpoint.
///
/// # Errors
/// - Returns [`CatalogError::CachePathNotFound`] when the environment variable is not set
///   and the platform cache directory cannot be determined.
/// - Returns [`CatalogError::Configuration`] when the environment variable is set but empty.
/// - Returns [`CatalogError::Io`] when cache file I/O fails without a usable fallback.
/// - Returns [`CatalogError::Reqwest`] when the HTTP request fails and no cache is available.
/// - Returns [`CatalogError::CacheFormat`] when the cache file is truncated, has length
///   mismatches, or contains invalid data.
/// - Returns [`CatalogError::Zstd`] when zstd decompression of cached data fails.
/// - Returns [`CatalogError::BitcodeDecode`] when the cached payload cannot be decoded.
/// - Returns [`CatalogError::ModelCatalogBuild`] when catalog reconstruction from cached
///   or downloaded data fails.
pub(crate) async fn load_catalog_at_path(path: &Path) -> CatalogResult<CatalogLoadResult> {
    let url = models_dev_api_url();
    load_catalog_from_url(path, url.as_ref()).await
}

#[maybe_async::maybe_async]
/// Synchronizes the cache at `path` against `url` and returns a catalog.
///
/// The sync flow is:
/// - read any existing cache file in one whole-file read
/// - send `If-None-Match` when the cache includes an ETag
/// - on `200 OK`, decode the response and rewrite the cache
/// - on `304 Not Modified`, load the existing cache
/// - on request, response-body, or transient status failure, fall back to cache when available
///
/// # Performance
///
/// Cache probing performs one up-front whole-file read through
/// [`read_cache_file`]. models.dev changes infrequently, so cache hits are
/// expected to be common, and [`crate::cache::payload`] documents typical
/// compressed payload sizes of about 23-32 kB. That makes a single sequential
/// read generally the faster hot path on modern NVMe-backed systems.
///
/// # Errors
/// - Returns [`CatalogError::Io`] when cache file read fails without a usable fallback.
/// - Returns [`CatalogError::CacheFormat`] when the existing cache file is truncated,
///   has size mismatches, or contains invalid prelude data.
/// - Returns [`CatalogError::Reqwest`] when the HTTP request fails and no valid cache
///   is available for fallback.
/// - Returns [`CatalogError::Json`] when the response body cannot be parsed as valid
///   models.dev API JSON.
/// - Returns [`CatalogError::ModelCatalogBuild`] when there are too many providers in
///   the API response or catalog reconstruction fails.
/// - Returns [`CatalogError::Zstd`] when compressing the payload for caching fails.
/// - Returns [`CatalogError::BitcodeDecode`] when decoding the cached or downloaded
///   payload fails.
/// - Returns [`CatalogError::Configuration`] when the server returns an unexpected
///   HTTP status code (not 200, 304, or a recognized transient status).
pub(crate) async fn load_catalog_from_url(
    path: &Path,
    url: &str,
) -> CatalogResult<CatalogLoadResult> {
    let mut cache_file = None;
    let mut cache_error = None;
    match read_cache_file(path).await {
        Ok(file) => cache_file = Some(file),
        Err(CatalogError::Io(error)) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => cache_error = Some(error),
    }

    #[cfg(feature = "tokio")]
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .expect("client builder should not fail with valid config");
    #[cfg(feature = "blocking")]
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .expect("client builder should not fail with valid config");

    let mut request = client.get(url);
    if let Some(etag) = cache_file.as_ref().and_then(|file| file.etag_bytes()) {
        request = request.header(IF_NONE_MATCH, etag);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return load_after_request_failure(error, cache_file.as_ref(), cache_error);
        }
    };
    match response.status() {
        StatusCode::OK => {
            let response_etag: Option<Vec<u8>> = response
                .headers()
                .get(ETAG)
                .map(|value| value.as_bytes().to_vec());
            let body = match response.bytes().await {
                Ok(body) => body,
                Err(error) => {
                    return load_after_request_failure(error, cache_file.as_ref(), cache_error);
                }
            };
            let payload = cache_payload_from_api_json_bytes(body.as_ref())?;
            let payload_encoded = encode_cache_payload(&payload);
            let catalog = catalog_from_cache_payload(payload)?;
            let payload_compressed = zstd::bulk::compress(payload_encoded.as_slice(), 17)
                .map_err(|error| CatalogError::Zstd(error.to_string()))?;

            write_cache_file(
                path,
                &CacheWriteInput {
                    etag: response_etag.as_deref(),
                    payload_compressed: &payload_compressed,
                    payload_len_decompressed: payload_encoded.len(),
                },
            )
            .await?;

            Ok(CatalogLoadResult {
                catalog,
                source: CatalogLoadSource::Downloaded,
            })
        }
        StatusCode::NOT_MODIFIED => {
            if let Some(cache_file) = cache_file.as_ref() {
                load_catalog_from_cache_file_data(cache_file, CatalogLoadSource::NotModifiedCache)
            } else if let Some(error) = cache_error {
                Err(error)
            } else {
                Err(CatalogError::CacheFormat(
                    "received 304 but no cached payload is available",
                ))
            }
        }
        status if is_transient_status(status) => {
            if let Some(cache_file) = cache_file.as_ref() {
                load_catalog_from_cache_file_data(cache_file, CatalogLoadSource::FallbackCache)
            } else if let Some(error) = cache_error {
                Err(error)
            } else {
                Err(CatalogError::Configuration(format!(
                    "unexpected catalog sync status: {status}",
                )))
            }
        }
        status => Err(CatalogError::Configuration(format!(
            "unexpected catalog sync status: {status}",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_utils::{sample_api_json, start_mock_server, MockResponse};
    use super::*;
    use crate::cache::format::CacheWriteInput;
    use crate::cache::payload::{
        encode_cache_payload, CachedModelRow, CachedProviderRow, CatalogCachePayload,
    };
    use llm_coding_tools_core::models::{Modality, ProviderIdx, ProviderType};
    use tempfile::TempDir;

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_downloaded_on_200() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("test.cache");

        // Start mock server returning 200 OK with fresh catalog data
        let body = String::from_utf8_lossy(sample_api_json()).to_string();
        let (_handle, url) = start_mock_server(MockResponse::Ok {
            etag: "\"test-etag-123\"",
            body,
        });

        let result = load_catalog_from_url(&cache_path, &url)
            .await
            .expect("sync should succeed");

        // Verify source is Downloaded (not from cache)
        assert_eq!(result.source, CatalogLoadSource::Downloaded);
        let provider = result
            .catalog
            .lookup_provider("openai")
            .expect("openai provider should exist");
        assert_eq!(provider.api_type, ProviderType::OpenAiCompletions);
        assert_eq!(provider.api_url, "https://api.openai.com/v1");

        // Verify cache file was written with the ETag from response
        let cache_file = read_cache_file(&cache_path)
            .await
            .expect("cache should exist");
        assert_eq!(
            cache_file.etag_bytes(),
            Some(b"\"test-etag-123\"".as_slice())
        );
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_cached_on_304_with_if_none_match() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("test.cache");

        // Pre-seed cache with a valid catalog payload
        let payload = CatalogCachePayload {
            providers: vec![CachedProviderRow {
                provider_key: "openai".to_string(),
                api_url: "https://api.openai.com/v1".to_string(),
                env_vars: vec!["OPENAI_API_KEY".to_string()],
                api_type: ProviderType::OpenAiCompletions,
            }],
            models: vec![CachedModelRow {
                provider_idx: ProviderIdx::new(0),
                model_key: "gpt-4".to_string(),
                modalities_bits: Modality::TEXT.bits(),
                max_input: 8192,
                max_output: 4096,
                temperature: None,
                top_p: None,
            }],
        };
        let encoded = encode_cache_payload(&payload);
        let compressed = zstd::bulk::compress(&encoded, 1).expect("compress");

        // Write the seeded cache file with ETag
        crate::cache::format::write_cache_file(
            &cache_path,
            &CacheWriteInput {
                etag: Some(b"\"cached-etag-456\""),
                payload_compressed: &compressed,
                payload_len_decompressed: encoded.len(),
            },
        )
        .await
        .expect("seed cache");

        // Server returns 304 Not Modified (ETag matches If-None-Match)
        let (_handle, url) = start_mock_server(MockResponse::NotModified {
            etag: "\"cached-etag-456\"",
        });

        let result = load_catalog_from_url(&cache_path, &url)
            .await
            .expect("sync should succeed");

        // Verify source is NotModifiedCache (loaded from local file)
        assert_eq!(result.source, CatalogLoadSource::NotModifiedCache);
        let provider = result
            .catalog
            .lookup_provider("openai")
            .expect("openai provider should exist");
        assert_eq!(provider.api_type, ProviderType::OpenAiCompletions);
    }

    fn refused_local_url() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        format!("http://127.0.0.1:{port}/api.json")
    }

    #[maybe_async::maybe_async]
    async fn seed_cache(cache_path: &Path) {
        let payload = CatalogCachePayload {
            providers: vec![CachedProviderRow {
                provider_key: "openai".to_string(),
                api_url: "https://api.openai.com/v1".to_string(),
                env_vars: vec!["OPENAI_API_KEY".to_string()],
                api_type: ProviderType::OpenAiCompletions,
            }],
            models: vec![CachedModelRow {
                provider_idx: ProviderIdx::new(0),
                model_key: "gpt-4".to_string(),
                modalities_bits: Modality::TEXT.bits(),
                max_input: 8192,
                max_output: 4096,
                temperature: None,
                top_p: None,
            }],
        };
        let encoded = encode_cache_payload(&payload);
        let compressed = zstd::bulk::compress(&encoded, 1).expect("compress");
        crate::cache::format::write_cache_file(
            cache_path,
            &CacheWriteInput {
                etag: Some(b"\"cached-etag-456\""),
                payload_compressed: &compressed,
                payload_len_decompressed: encoded.len(),
            },
        )
        .await
        .expect("seed cache");
    }

    #[test]
    fn transient_status_detection_matches_retryable_responses() {
        assert!(is_transient_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_transient_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!is_transient_status(StatusCode::NOT_FOUND));
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_fallback_cache_on_request_failure_with_valid_cache() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("test.cache");

        seed_cache(&cache_path).await;

        let result = load_catalog_from_url(&cache_path, &refused_local_url())
            .await
            .expect("fallback should succeed");

        assert_eq!(result.source, CatalogLoadSource::FallbackCache);
        assert!(result.catalog.lookup_provider("openai").is_some());
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_fallback_cache_on_transient_status_with_valid_cache() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("test.cache");

        seed_cache(&cache_path).await;

        let (_handle, url) = start_mock_server(MockResponse::Status {
            code: 503,
            reason: "Service Unavailable",
        });

        let result = load_catalog_from_url(&cache_path, &url)
            .await
            .expect("fallback should succeed");

        assert_eq!(result.source, CatalogLoadSource::FallbackCache);
        assert!(result.catalog.lookup_provider("openai").is_some());
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_cache_error_on_transient_status_with_corrupt_cache() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("corrupt.cache");

        std::fs::write(&cache_path, [0_u8; 11]).expect("write corrupt cache");

        let (_handle, url) = start_mock_server(MockResponse::Status {
            code: 429,
            reason: "Too Many Requests",
        });

        match load_catalog_from_url(&cache_path, &url).await {
            Err(error) => assert!(matches!(error, CatalogError::CacheFormat(_))),
            Ok(_) => panic!("transient status with corrupt cache should error"),
        }
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_fallback_cache_on_body_read_failure_with_valid_cache() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("test.cache");

        seed_cache(&cache_path).await;

        let (_handle, url) = start_mock_server(MockResponse::PartialOk {
            etag: "\"fresh-etag\"",
            body: "{".to_string(),
            content_length: 32,
        });

        let result = load_catalog_from_url(&cache_path, &url)
            .await
            .expect("fallback should succeed");

        assert_eq!(result.source, CatalogLoadSource::FallbackCache);
        assert!(result.catalog.lookup_provider("openai").is_some());
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_request_error_when_request_fails_without_cache() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("missing.cache");

        match load_catalog_from_url(&cache_path, &refused_local_url()).await {
            Err(error) => assert!(matches!(error, CatalogError::Reqwest(_))),
            Ok(_) => panic!("request failure without cache should error"),
        }
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn sync_returns_cache_error_when_request_fails_with_corrupt_cache() {
        let temp = TempDir::new().expect("tempdir");
        let cache_path = temp.path().join("corrupt.cache");

        std::fs::write(&cache_path, [0_u8; 11]).expect("write corrupt cache");

        match load_catalog_from_url(&cache_path, &refused_local_url()).await {
            Err(error) => assert!(matches!(error, CatalogError::CacheFormat(_))),
            Ok(_) => panic!("request failure with corrupt cache should error"),
        }
    }
}
