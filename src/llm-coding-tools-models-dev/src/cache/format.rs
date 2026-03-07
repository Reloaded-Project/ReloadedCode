//! Cache container layout and read/write helpers.
//!
//! The on-disk layout for `models.dev.catalog.v1.cache` is:
//!
//! ```text
//! [0..12)   12-byte fixed prelude:
//!           - [0..4)   etag_len: u32 little-endian
//!           - [4..8)   payload_len_compressed: u32 little-endian
//!           - [8..12)  payload_len_decompressed: u32 little-endian
//! [12..N)   raw ETag bytes (etag_len bytes, may be 0)
//! [N..EOF)  compressed payload (rest of file)
//! ```
//!
//! Versioning is keyed by filename (`*.v1.cache`), so this prelude carries
//! lengths only and no magic marker.
//! `payload_len_compressed` is retained so reads can detect unexpected file
//! truncation before decode.
//!
//! Read path intentionally keeps payload compressed. We read the whole file in
//! one pre-sized allocation, then parse/slice into `prelude`, `etag`, and
//! `payload` views without additional copying.
//!
//! ## Performance
//!
//! models.dev changes infrequently, so cache hits are expected to be common.
//! [`crate::cache::payload`] documents typical compressed payload sizes of about
//! 23-32 kB, which keeps the whole container small enough that a single
//! sequential read is generally the faster, simpler hot path on modern
//! NVMe-backed systems.
//!
//! ## Safety
//!
//! Not a 'safe' parser. We assume the file was created by the user.
//! There's no validation for erroneous data; e.g. maliciously crafted headers.
//! Only validation for accidental corruption/truncation (e.g., from partial writes) is included.

use crate::{
    error::{CatalogError, CatalogResult},
    fs,
};
use endian_writer::{EndianReader, EndianWriter, HasSize, LittleEndianReader, LittleEndianWriter};
use endian_writer_derive::EndianWritable;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::copy_nonoverlapping;

/// Fixed v1 prelude, encoded little-endian.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EndianWritable)]
#[repr(C)]
struct CachePreludeV1 {
    /// Length in bytes of the optional ETag block.
    etag_len: u32,
    /// Length in bytes of compressed payload as written to disk.
    payload_len_compressed: u32,
    /// Length in bytes after decompression.
    payload_len_decompressed: u32,
}

/// Input parameters for writing a cache container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheWriteInput<'a> {
    /// Optional ETag bytes (e.g., HTTP ETag value).
    pub(crate) etag: Option<&'a [u8]>,
    /// Compressed payload bytes.
    pub(crate) payload_compressed: &'a [u8],
    /// Expected decompressed payload length in bytes.
    pub(crate) payload_len_decompressed: usize,
}

/// Fixed prelude size for v1.
const CACHE_HEADER_LEN: usize = <CachePreludeV1 as HasSize>::SIZE;

// SAFETY: All modern platforms have usize >= 32 bits.
// This lets us safely cast u32 lengths to usize without checked arithmetic.
const _: () = assert!(size_of::<usize>() >= size_of::<u32>());

/// Raw cache blocks extracted from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheFileData {
    /// Prefix length of ETag bytes after the fixed prelude.
    etag_len: u32,
    /// Length in bytes of compressed payload from prelude.
    payload_len_compressed: u32,
    /// Size hint for the eventual decompressed payload allocation.
    payload_len_decompressed: u32,
    /// Full file bytes laid out as `prelude || etag || payload_compressed`.
    file_bytes: Box<[u8]>,
}

/// Returns a temporary path for atomic cache writes.
fn temp_cache_path(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".tmp");
    PathBuf::from(temp)
}

impl CacheFileData {
    /// Returns the optional ETag as a borrowed byte slice.
    #[inline]
    pub(crate) fn etag_bytes(&self) -> Option<&[u8]> {
        let etag_start = CACHE_HEADER_LEN;
        let etag_end = CACHE_HEADER_LEN + self.etag_len as usize;
        let etag = &self.file_bytes[etag_start..etag_end];
        if etag.is_empty() {
            None
        } else {
            Some(etag)
        }
    }

    /// Returns compressed payload bytes as a borrowed slice.
    #[inline]
    pub(crate) fn payload_compressed(&self) -> &[u8] {
        let payload_start = CACHE_HEADER_LEN + self.etag_len as usize;
        &self.file_bytes[payload_start..]
    }

    /// Returns compressed payload length in bytes.
    #[inline]
    pub(crate) fn payload_len_compressed(&self) -> u32 {
        self.payload_len_compressed
    }

    /// Returns expected decompressed payload length in bytes.
    #[inline]
    pub(crate) fn payload_len_decompressed(&self) -> u32 {
        self.payload_len_decompressed
    }
}

/// Reads a cache container from disk.
///
/// This reads the entire cache file into memory in one shot, then parses only
/// the prelude + raw blocks and does not decompress payload.
/// Compressed payload length is validated against prelude metadata to catch
/// unexpected truncation or trailing bytes before decode.
///
/// # Performance
///
/// This intentionally performs one whole-file read. models.dev changes
/// infrequently, so cache hits are expected to be common, and
/// [`crate::cache::payload`] documents typical compressed payload sizes of about
/// 23-32 kB. That is generally faster in practice than a streaming path while
/// remaining effectively negligible on modern NVMe-backed systems.
///
/// # Errors
///
/// Returns [`CatalogError::CacheFormat`] when the prelude is truncated, when
/// encoded lengths overflow platform limits, or when declared block lengths do not
/// match file contents.
#[maybe_async::maybe_async]
pub(crate) async fn read_cache_file(path: &Path) -> CatalogResult<CacheFileData> {
    let file_bytes = fs::read(path).await?;
    if file_bytes.len() < CACHE_HEADER_LEN {
        return Err(CatalogError::CacheFormat("cache prelude is truncated"));
    }

    let prelude = decode_prelude(&file_bytes[..CACHE_HEADER_LEN]);
    let etag_len = prelude.etag_len as usize;
    let payload_len_compressed = prelude.payload_len_compressed as usize;
    let expected_total = CACHE_HEADER_LEN + etag_len + payload_len_compressed; // unlikely to overflow. file is trusted.

    if file_bytes.len() != expected_total {
        return Err(CatalogError::CacheFormat(
            "cache file size mismatch (possible truncation or trailing data)",
        ));
    }

    Ok(CacheFileData {
        etag_len: prelude.etag_len,
        payload_len_compressed: prelude.payload_len_compressed,
        payload_len_decompressed: prelude.payload_len_decompressed,
        file_bytes,
    })
}

/// Writes a cache container to disk.
///
/// # Errors
///
/// Returns [`CatalogError::CacheFormat`] if a block length exceeds v1 `u32`
/// limits.
#[maybe_async::maybe_async]
pub(crate) async fn write_cache_file(
    path: &Path,
    input: &CacheWriteInput<'_>,
) -> CatalogResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let etag_bytes = input.etag.unwrap_or(&[]);
    let prelude = CachePreludeV1 {
        etag_len: to_u32_limit(etag_bytes.len(), "etag exceeds v1 length limits")?,
        payload_len_compressed: to_u32_limit(
            input.payload_compressed.len(),
            "compressed payload exceeds v1 length limits",
        )?,
        payload_len_decompressed: to_u32_limit(
            input.payload_len_decompressed,
            "decompressed payload exceeds v1 length limits",
        )?,
    };

    let encoded_prelude = encode_prelude(prelude);

    let encoded_len = CACHE_HEADER_LEN
        .checked_add(etag_bytes.len())
        .and_then(|value| value.checked_add(input.payload_compressed.len()))
        .ok_or(CatalogError::CacheFormat(
            "cache file exceeds platform length limits",
        ))?;

    let mut uninit = fs::alloc_uninit_u8_slice(encoded_len);
    let ptr = uninit.as_mut_ptr().cast::<u8>();

    unsafe {
        copy_nonoverlapping(encoded_prelude.as_ptr(), ptr, CACHE_HEADER_LEN);
        copy_nonoverlapping(
            etag_bytes.as_ptr(),
            ptr.add(CACHE_HEADER_LEN),
            etag_bytes.len(),
        );
        copy_nonoverlapping(
            input.payload_compressed.as_ptr(),
            ptr.add(CACHE_HEADER_LEN + etag_bytes.len()),
            input.payload_compressed.len(),
        );
    }

    let file_bytes = fs::assume_init_u8_slice(uninit);
    let temp_path = temp_cache_path(path);
    fs::write(&temp_path, &file_bytes).await?;
    fs::rename(&temp_path, path).await?;
    Ok(())
}

#[inline]
fn to_u32_limit(value: usize, msg: &'static str) -> CatalogResult<u32> {
    u32::try_from(value).map_err(|_| CatalogError::CacheFormat(msg))
}

/// Encodes prelude into little-endian bytes.
#[inline]
fn encode_prelude(prelude: CachePreludeV1) -> [u8; CACHE_HEADER_LEN] {
    let mut bytes = [0_u8; CACHE_HEADER_LEN];
    // SAFETY: `bytes` has exactly the derived serialized size of `CachePreludeV1`.
    unsafe {
        let mut writer = LittleEndianWriter::new(bytes.as_mut_ptr());
        writer.write(&prelude);
    }
    bytes
}

/// Decodes prelude from little-endian bytes.
#[inline]
fn decode_prelude(bytes: &[u8]) -> CachePreludeV1 {
    // SAFETY: Caller guarantees `bytes` is at least `CACHE_HEADER_LEN`.
    unsafe {
        let mut reader = LittleEndianReader::new(bytes.as_ptr());
        reader.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Verifies prelude encoding/decoding preserves all fields.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn prelude_layout_round_trips() {
        let prelude = CachePreludeV1 {
            etag_len: 13,
            payload_len_compressed: 44,
            payload_len_decompressed: 333,
        };

        let round_trip = decode_prelude(&encode_prelude(prelude));
        assert_eq!(round_trip, prelude);
    }

    // Verifies full round-trip with ETag included.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_then_read_round_trips_with_etag() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("models.dev.catalog.v1.cache");

        let input = CacheWriteInput {
            etag: Some(b"etag-123"),
            payload_compressed: b"payload-zstd-bytes",
            payload_len_decompressed: 2048,
        };
        write_cache_file(&path, &input).await.expect("write cache");
        let data = read_cache_file(&path).await.expect("read cache");

        assert_eq!(data.etag_bytes(), input.etag);
        assert_eq!(data.payload_compressed(), input.payload_compressed);
        assert_eq!(
            data.payload_len_compressed(),
            input.payload_compressed.len() as u32
        );
        assert_eq!(
            data.payload_len_decompressed(),
            input.payload_len_decompressed as u32
        );
    }

    // Verifies full round-trip without ETag.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_then_read_round_trips_without_etag() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("models.dev.catalog.v1.cache");

        let input = CacheWriteInput {
            etag: None,
            payload_compressed: b"payload-only",
            payload_len_decompressed: 1024,
        };
        write_cache_file(&path, &input).await.expect("write cache");
        let data = read_cache_file(&path).await.expect("read cache");

        assert_eq!(data.etag_bytes(), input.etag);
        assert_eq!(data.payload_compressed(), input.payload_compressed);
        assert_eq!(
            data.payload_len_decompressed(),
            input.payload_len_decompressed as u32
        );
    }

    // Rejects files shorter than the fixed header.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_rejects_truncated_prelude() {
        // File is 1 byte shorter than required header
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("short-prelude.cache");

        std::fs::write(&path, [0_u8; CACHE_HEADER_LEN - 1]).expect("write fixture");
        let error = read_cache_file(&path)
            .await
            .expect_err("truncated prelude should fail");
        assert!(matches!(error, CatalogError::CacheFormat(_)));
    }

    // Rejects when file ends before etag_len bytes after header.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_rejects_short_etag_length() {
        // Header claims 12 bytes of etag but only 4 provided
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("short-etag.cache");

        let prelude = CachePreludeV1 {
            etag_len: 12,
            payload_len_compressed: 0,
            payload_len_decompressed: 0,
        };
        let mut bytes = encode_prelude(prelude).to_vec();
        bytes.extend_from_slice(b"tiny"); // 'tiny' etag is 4 bytes
        std::fs::write(&path, bytes).expect("write fixture");

        // Header claims 12 bytes of etag but only 4 'tiny' provided, so 8 bytes short.
        let error = read_cache_file(&path)
            .await
            .expect_err("short etag should fail");
        assert!(matches!(error, CatalogError::CacheFormat(_)));
    }

    // Accepts minimal valid file with all zero-length fields.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_supports_empty_etag_and_payload() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("empty.cache");

        let prelude = CachePreludeV1 {
            etag_len: 0,
            payload_len_compressed: 0,
            payload_len_decompressed: 0,
        };
        std::fs::write(&path, encode_prelude(prelude)).expect("write fixture");
        let data = read_cache_file(&path).await.expect("read empty cache");

        assert_eq!(data.etag_bytes(), None);
        assert!(data.payload_compressed().is_empty());
        assert_eq!(data.payload_len_compressed(), 0);
        assert_eq!(data.payload_len_decompressed(), 0);
    }

    // Rejects when declared compressed payload length does not match file size.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_rejects_mismatched_payload_length() {
        // Header claims 10 bytes payload but only 5 provided
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("mismatched-payload-len.cache");

        let prelude = CachePreludeV1 {
            etag_len: 4,
            payload_len_compressed: 10,
            payload_len_decompressed: 0,
        };
        let mut bytes = encode_prelude(prelude).to_vec();
        bytes.extend_from_slice(b"etag");
        bytes.extend_from_slice(b"short"); // only 5 bytes, not 10 here.
        std::fs::write(&path, bytes).expect("write fixture");

        let error = read_cache_file(&path)
            .await
            .expect_err("payload length mismatch should fail");
        assert!(matches!(error, CatalogError::CacheFormat(_)));
    }

    // Verifies atomic replacement replaces existing cache file content.
    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn write_replaces_existing_cache_atomically() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("atomic-test.cache");

        // Write first payload
        let first_input = CacheWriteInput {
            etag: Some(b"etag-1"),
            payload_compressed: b"first-payload",
            payload_len_decompressed: 100,
        };
        write_cache_file(&path, &first_input)
            .await
            .expect("write first");

        let first_data = read_cache_file(&path).await.expect("read first");
        assert_eq!(first_data.etag_bytes(), Some(b"etag-1".as_slice()));
        assert_eq!(first_data.payload_compressed(), b"first-payload");

        // Write second payload (atomic replacement)
        let second_input = CacheWriteInput {
            etag: Some(b"etag-2"),
            payload_compressed: b"second-payload-different",
            payload_len_decompressed: 200,
        };
        write_cache_file(&path, &second_input)
            .await
            .expect("write second");

        let second_data = read_cache_file(&path).await.expect("read second");
        assert_eq!(second_data.etag_bytes(), Some(b"etag-2".as_slice()));
        assert_eq!(
            second_data.payload_compressed(),
            b"second-payload-different"
        );
        assert_eq!(second_data.payload_len_decompressed(), 200);
    }
}
