//! Packed `ProviderTable` key -> provider-index entry.
//!
//! Layout (`u64`):
//! - `48` bits: truncated provider hash
//! - `16` bits: provider index

use crate::models::catalog::public::ProviderIdx;
use bitfields::bitfield;

/// Number of retained hash bits for provider lookup entries.
pub const PROVIDER_TABLE_HASH_BITS: u32 = 48;
/// Bitmask used to truncate hashes to 48 bits.
pub const PROVIDER_TABLE_HASH_MASK: u64 = (1u64 << PROVIDER_TABLE_HASH_BITS) - 1;

/// Maximum provider index representable by `u16`.
pub const MAX_PROVIDER_IDX: u16 = u16::MAX;
/// Maximum provider count representable by `u16` indices.
pub const MAX_PROVIDER_COUNT: usize = (MAX_PROVIDER_IDX as usize) + 1;

const _: () = assert!(PROVIDER_TABLE_HASH_BITS + 16 == 64);

/// Packed provider-table entry.
#[bitfield(u64)]
#[derive(PartialEq, Eq, Hash)]
pub struct PackedProviderTableEntry {
    #[bits(48)]
    hash48: u64,
    provider_idx: u16,
}

impl PackedProviderTableEntry {
    /// Truncates a 64-bit hash to the retained 48-bit key.
    #[inline]
    pub const fn truncate_hash48(hash: u64) -> u64 {
        hash & PROVIDER_TABLE_HASH_MASK
    }

    /// Creates one packed provider-table entry.
    #[inline]
    pub fn from_parts(hash64: u64, provider_idx: u16) -> Self {
        let mut packed = Self::new_without_defaults();
        packed.set_hash48(Self::truncate_hash48(hash64));
        packed.set_provider_idx(provider_idx);
        packed
    }

    /// Creates one packed provider-table entry using [`ProviderIdx`].
    #[inline]
    pub fn from_parts_idx(hash64: u64, provider_idx: ProviderIdx) -> Self {
        Self::from_parts(hash64, provider_idx.as_u16())
    }

    /// Returns the provider index as a [`ProviderIdx`].
    #[inline]
    pub fn provider_idx_val(self) -> ProviderIdx {
        ProviderIdx::new(self.provider_idx())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_provider_table_entry_is_8_bytes() {
        assert_eq!(core::mem::size_of::<PackedProviderTableEntry>(), 8);
    }

    #[test]
    fn truncate_hash48_masks_to_48_bits() {
        let full = 0xFFFF_FFFF_FFFF_FFFFu64;
        assert_eq!(
            PackedProviderTableEntry::truncate_hash48(full),
            PROVIDER_TABLE_HASH_MASK
        );
    }

    #[test]
    fn provider_idx_roundtrips() {
        let packed = PackedProviderTableEntry::from_parts(0xDEAD_BEEF_F00D_CAFEu64, 7);
        assert_eq!(packed.provider_idx(), 7);
    }

    #[test]
    fn provider_idx_val_roundtrips() {
        let idx = ProviderIdx::new(7);
        let packed = PackedProviderTableEntry::from_parts_idx(0xDEAD_BEEF_F00D_CAFEu64, idx);
        assert_eq!(packed.provider_idx_val(), idx);
    }
}
