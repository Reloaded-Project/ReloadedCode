//! Filesystem abstraction layer.
//!
//! Provides unified APIs that work with both sync and async runtimes.
//! Exactly one runtime feature must be enabled:
//! - `tokio`: Async operations using the tokio runtime
//! - `blocking`: Synchronous operations

use std::mem::MaybeUninit;

#[cfg(all(feature = "tokio", feature = "blocking"))]
compile_error!("Features `tokio` and `blocking` are mutually exclusive.");

#[cfg(not(any(feature = "tokio", feature = "blocking")))]
compile_error!("Either `tokio` or `blocking` feature must be enabled for the fs module.");

/// Allocates an uninitialized boxed byte slice with logical length `len`.
#[inline]
pub(crate) fn alloc_uninit_u8_slice(len: usize) -> Box<[MaybeUninit<u8>]> {
    Box::<[u8]>::new_uninit_slice(len)
}

/// Views an uninitialized `u8` slice as mutable bytes for initialization.
#[inline]
pub(crate) fn uninit_u8_slice_as_mut_bytes(bytes: &mut [MaybeUninit<u8>]) -> &mut [u8] {
    // SAFETY: `MaybeUninit<u8>` has identical layout to `u8`; caller only uses
    // returned slice for writes before reading.
    unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast::<u8>(), bytes.len()) }
}

/// Converts a fully-initialized boxed uninitialized slice into initialized bytes.
#[inline]
pub(crate) fn assume_init_u8_slice(bytes: Box<[MaybeUninit<u8>]>) -> Box<[u8]> {
    // SAFETY: caller guarantees all bytes were initialized.
    unsafe { bytes.assume_init() }
}

#[cfg(feature = "tokio")]
mod tokio_impl;
#[cfg(feature = "tokio")]
pub(crate) use tokio_impl::*;

#[cfg(feature = "blocking")]
mod blocking_impl;
#[cfg(feature = "blocking")]
pub(crate) use blocking_impl::*;
