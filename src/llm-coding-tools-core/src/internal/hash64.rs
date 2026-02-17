//! Wrapper for an internal 64-bit hash.
//!
//! Currently uses ahash64 under the hood, based on performance requirements
//! (handling short strings, while also scaling well); but given this is an
//! internal type, that's an implementation detail.

/// A 64-bit hash value using the ahash algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Hash64(u64);

impl Hash64 {
    /// Creates a new Hash64 from a raw u64 value.
    #[inline]
    #[allow(dead_code)] // internal public API
    pub(crate) fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying u64 value.
    #[inline]
    #[allow(dead_code)] // internal public API
    pub(crate) fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Hashes a string to Hash64 using ahash64.
#[inline(always)]
#[allow(dead_code)] // internal public API
pub(crate) fn hash_u64(s: &str) -> Hash64 {
    hash_u64_bytes(s.as_bytes())
}

/// Hashes raw bytes to Hash64 using ahash64.
#[inline(always)]
#[allow(dead_code)] // internal public API
pub(crate) fn hash_u64_bytes(bytes: &[u8]) -> Hash64 {
    Hash64(ahash::RandomState::with_seed(0xDEAD_CAFE).hash_one(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let hash1 = hash_u64("bash");
        let hash2 = hash_u64("bash");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn different_inputs_produce_different_hashes() {
        let h1 = hash_u64("bash");
        let h2 = hash_u64("read");
        let h3 = hash_u64("write");
        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
        assert_ne!(h2, h3);
    }
}
