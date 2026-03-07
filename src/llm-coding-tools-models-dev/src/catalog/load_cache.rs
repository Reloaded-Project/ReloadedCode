//! Loading a model catalog from cached on-disk data.
//!
//! This module handles the offline half of catalog loading: it decompresses the
//! stored payload, decodes the serialized rows, and rebuilds a
//! [`ModelCatalog`](llm_coding_tools_core::models::ModelCatalog).

use crate::cache::format::CacheFileData;
use crate::cache::payload::{catalog_from_cache_payload, decode_cache_payload};
use crate::catalog::load_result::{CatalogLoadResult, CatalogLoadSource};
use crate::error::{CatalogError, CatalogResult};

/// Decompresses cache file data and rebuilds a catalog from it.
///
/// # Errors
///
/// Returns [`CatalogError`] when zstd decompression fails, the decompressed
/// length does not match the cache metadata, the serialized payload cannot be
/// decoded, or catalog reconstruction fails.
pub(crate) fn load_catalog_from_cache_file_data(
    cache_file: &CacheFileData,
    source: CatalogLoadSource,
) -> CatalogResult<CatalogLoadResult> {
    let expected_len = cache_file.payload_len_decompressed() as usize;
    let decoded = zstd::bulk::decompress(cache_file.payload_compressed(), expected_len)
        .map_err(|error| CatalogError::Zstd(error.to_string()))?;
    if decoded.len() != expected_len {
        return Err(CatalogError::CacheFormat(
            "cache payload length mismatch after decompression",
        ));
    }

    let payload = decode_cache_payload(&decoded)?;
    let catalog = catalog_from_cache_payload(payload)?;
    Ok(CatalogLoadResult { catalog, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::format::{write_cache_file, CacheWriteInput};
    use crate::cache::payload::{
        encode_cache_payload, CachedModelRow, CachedProviderRow, CatalogCachePayload,
    };
    use llm_coding_tools_core::models::{Modality, ProviderIdx, ProviderType};
    use tempfile::TempDir;

    fn sample_payload() -> CatalogCachePayload {
        CatalogCachePayload {
            providers: vec![CachedProviderRow {
                provider_key: "test".to_string(),
                api_url: "https://test.example".to_string(),
                env_vars: vec![],
                api_type: ProviderType::OpenAiCompletions,
            }],
            models: vec![CachedModelRow {
                provider_idx: ProviderIdx::new(0),
                model_key: "model1".to_string(),
                modalities_bits: Modality::TEXT.bits(),
                max_input: 4096,
                max_output: 2048,
                temperature: None,
                top_p: None,
            }],
        }
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn round_trip_through_cache_file() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("test.cache");

        let payload = sample_payload();
        let encoded = encode_cache_payload(&payload);
        let compressed =
            zstd::bulk::compress(&encoded, 1).expect("compress");

        write_cache_file(
            &path,
            &CacheWriteInput {
                etag: Some(b"test-etag"),
                payload_compressed: &compressed,
                payload_len_decompressed: encoded.len(),
            },
        )
        .await
        .expect("write cache");

        let cache_file = crate::cache::format::read_cache_file(&path)
            .await
            .expect("read cache");
        let result =
            load_catalog_from_cache_file_data(&cache_file, CatalogLoadSource::NotModifiedCache)
                .expect("load from cache");

        assert_eq!(result.source, CatalogLoadSource::NotModifiedCache);
        let provider = result
            .catalog
            .lookup_provider("test")
            .expect("provider should exist");
        assert_eq!(provider.api_type, ProviderType::OpenAiCompletions);
    }
}
