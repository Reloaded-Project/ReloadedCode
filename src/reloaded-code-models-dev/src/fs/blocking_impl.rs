//! Blocking/sync filesystem operations.

use std::io::{ErrorKind, Read as _};
use std::path::Path;

/// Reads a file into memory in one pre-sized allocation.
///
/// # Safety
///
/// We snapshot file length then call `read_exact`, which would miss data appended after
/// the metadata call if the file grew mid-read. However, within this codebase all
/// writes go to a temp file first, then rename to target - so files are never
/// appended to in place.
/// Therefore this race cannot occur.
#[inline]
pub(crate) fn read(path: impl AsRef<Path>) -> std::io::Result<Box<[u8]>> {
    let mut file = std::fs::File::open(path)?;
    let file_len_u64 = file.metadata()?.len();
    let file_len = usize::try_from(file_len_u64).map_err(|_| {
        std::io::Error::new(ErrorKind::InvalidData, "file is too large to fit in memory")
    })?;

    let mut bytes = super::alloc_uninit_u8_slice(file_len);
    if file_len != 0 {
        let buf = super::uninit_u8_slice_as_mut_bytes(&mut bytes);
        file.read_exact(buf)?;
    }

    Ok(super::assume_init_u8_slice(bytes))
}

/// Creates a directory and all parent directories.
#[inline]
pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}
