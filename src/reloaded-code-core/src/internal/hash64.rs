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
    pub(crate) fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Returns the underlying u64 value.
    #[inline]
    pub(crate) fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Hashes a string to Hash64 using ahash64.
#[inline(always)]
pub(crate) fn hash_u64(s: &str) -> Hash64 {
    hash_u64_bytes(s.as_bytes())
}

/// Hashes raw bytes to Hash64 using ahash64.
#[inline(always)]
pub(crate) fn hash_u64_bytes(bytes: &[u8]) -> Hash64 {
    Hash64(ahash::RandomState::with_seed(0xDEAD_CAFE).hash_one(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// Verifies that the hash function is deterministic for identical inputs
    /// and produces different hashes for different inputs.
    #[rstest]
    #[case::same_input("bash", "bash", true)]
    #[case::different_inputs("bash", "read", false)]
    #[case::different_inputs_2("bash", "write", false)]
    #[case::different_inputs_3("read", "write", false)]
    fn hash_properties(#[case] a: &str, #[case] b: &str, #[case] should_equal: bool) {
        let hash_a = hash_u64(a);
        let hash_b = hash_u64(b);
        if should_equal {
            assert_eq!(hash_a, hash_b);
        } else {
            assert_ne!(hash_a, hash_b);
        }
    }
}
