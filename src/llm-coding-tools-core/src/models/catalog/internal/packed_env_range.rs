//! Packed env-var range entry for provider-to-env-key mapping.
//!
//! Layout (`u16`):
//! - `14` bits: start index into provider_env_keys StringTable
//! - `2` bits: count of env keys for this provider (0..=3)

use bitfields::bitfield;

/// Maximum env-var count per provider representable by PackedEnvRange.
pub const MAX_ENV_RANGE_COUNT: u8 = 3;
/// Maximum start index representable by PackedEnvRange (14 bits).
pub const MAX_ENV_START: u16 = (1u16 << 14) - 1; // 16383

/// Packed env-var range entry.
///
/// Stores (start, count) for env keys in the provider_env_keys StringTable.
#[bitfield(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedEnvRange {
    #[bits(14)]
    start: u16,
    #[bits(2)]
    count: u8,
}

impl PackedEnvRange {
    /// Creates one packed env-var range entry.
    ///
    /// SAFETY: The `start` parameter is not validated here. The caller must ensure
    /// `start` fits within 14 bits (max 16383). This invariant is enforced in
    /// `analyze_provider_sources` before `populate_tables_once` calls this function.
    #[inline]
    pub fn from_parts(start: u16, count: u8) -> Self {
        let mut packed = Self::new_without_defaults();
        packed.set_start(start);
        packed.set_count(count.min(MAX_ENV_RANGE_COUNT));
        packed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_env_range_is_2_bytes() {
        assert_eq!(core::mem::size_of::<PackedEnvRange>(), 2);
    }

    #[test]
    fn env_range_roundtrip() {
        let packed = PackedEnvRange::from_parts(1234, 2);
        assert_eq!(packed.start(), 1234);
        assert_eq!(packed.count(), 2);
    }

    #[test]
    fn count_capped_at_max() {
        let packed = PackedEnvRange::from_parts(0, 5);
        assert_eq!(packed.count(), 3); // capped to MAX_ENV_RANGE_COUNT
    }
}
