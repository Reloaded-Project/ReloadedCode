//! 63-bit hash type.
//!
//! A 63-bit hash value representing the upper 63 bits of an original hash.
//! More specifically, a 64-bit hash value which has been `>> 1`.
//! i.e. 64th bit is always 0.

/// A 63-bit hash value representing the upper 63 bits of an original hash.
/// Result of right shifting a hash `>> 1`.
///
/// # Bit Layout
/// - Bits 0-62: Original hash bits 1-63 (upper 63 bits)
/// - Bit  63: Always 0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Hash63(u64);

impl Hash63 {
    /// Creates a new Hash63 from a raw u64 value.
    ///
    /// The caller is responsible for ensuring bit 63 is 0.
    #[inline]
    #[allow(dead_code)] // internal public API
    pub(crate) const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying u64 value.
    #[inline]
    #[allow(dead_code)] // internal public API
    pub(crate) const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Creates a Hash63 from a Hash64 by extracting upper 63 bits.
    #[inline]
    pub(crate) fn from_hash64(hash: crate::internal::hash64::Hash64) -> Self {
        Self(hash.as_u64() >> 1)
    }
}
