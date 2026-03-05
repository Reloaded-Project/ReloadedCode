//! Packed `ProviderModelTable` key -> model-configuration-index entry.
//!
//! Layout (`u64`):
//! - `48` bits: truncated provider-model hash
//! - `16` bits: model-configuration index

use crate::models::catalog::public::ModelIdx;
use bitfields::bitfield;

/// Number of retained hash bits for provider-model lookup entries.
pub const PROVIDER_MODEL_TABLE_HASH_BITS: u32 = 48;
/// Bitmask used to truncate hashes to 48 bits.
pub const PROVIDER_MODEL_TABLE_HASH_MASK: u64 = (1u64 << PROVIDER_MODEL_TABLE_HASH_BITS) - 1;

/// Maximum model-configuration index representable by `u16`.
pub const MAX_MODEL_CONFIG_IDX: u16 = u16::MAX;
/// Maximum model-configuration entry count representable by `u16`.
pub const MAX_MODEL_CONFIG_COUNT: usize = (MAX_MODEL_CONFIG_IDX as usize) + 1;

const _: () = assert!(PROVIDER_MODEL_TABLE_HASH_BITS + 16 == 64);

/// Packed provider-model-table entry.
#[bitfield(u64)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedProviderModelTableEntry {
    #[bits(48)]
    hash48: u64,
    model_config_idx: u16,
}

impl PackedProviderModelTableEntry {
    /// Truncates a 64-bit hash to the retained 48-bit key.
    #[inline]
    pub const fn truncate_hash48(hash: u64) -> u64 {
        hash & PROVIDER_MODEL_TABLE_HASH_MASK
    }

    /// Creates one packed provider-model-table entry.
    #[inline]
    pub fn from_parts(hash64: u64, model_config_idx: u16) -> Self {
        let mut packed = Self::new_without_defaults();
        packed.set_hash48(Self::truncate_hash48(hash64));
        packed.set_model_config_idx(model_config_idx);
        packed
    }

    /// Creates one packed provider-model-table entry using [`ModelIdx`].
    #[inline]
    pub fn from_parts_idx(hash64: u64, model_config_idx: ModelIdx) -> Self {
        Self::from_parts(hash64, model_config_idx.as_u16())
    }

    /// Returns the model configuration index as a [`ModelIdx`].
    #[inline]
    pub fn model_config_idx_val(self) -> ModelIdx {
        ModelIdx::new(self.model_config_idx())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_provider_model_table_entry_is_8_bytes() {
        assert_eq!(core::mem::size_of::<PackedProviderModelTableEntry>(), 8);
    }

    #[test]
    fn truncate_hash48_masks_to_48_bits() {
        let full = 0xFFFF_FFFF_FFFF_FFFFu64;
        assert_eq!(
            PackedProviderModelTableEntry::truncate_hash48(full),
            PROVIDER_MODEL_TABLE_HASH_MASK
        );
    }

    #[test]
    fn model_config_idx_roundtrips() {
        let packed = PackedProviderModelTableEntry::from_parts(0xDEAD_BEEF_F00D_CAFEu64, 42);
        assert_eq!(packed.model_config_idx(), 42);
    }

    #[test]
    fn model_config_idx_val_roundtrips() {
        let idx = ModelIdx::new(42);
        let packed = PackedProviderModelTableEntry::from_parts_idx(0xDEAD_BEEF_F00D_CAFEu64, idx);
        assert_eq!(packed.model_config_idx_val(), idx);
    }
}
