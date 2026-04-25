//! Packed model metadata entry.
//!
//! Layout (`u64`):
//! - `8` bits: modality bitflags
//! - `27` bits: max output tokens
//! - `29` bits: max input tokens

use crate::models::catalog::{Modality, ModelInfo};
use bitfields::bitfield;

/// Number of bits allocated to modality flags.
pub const MODALITY_BITS: u32 = 8;
/// Number of bits allocated to max output tokens.
pub const MAX_OUTPUT_BITS: u32 = 27;
/// Number of bits allocated to max input tokens.
pub const MAX_INPUT_BITS: u32 = 29;

/// Maximum output token value representable by 27 bits (`134_217_727`).
pub const MAX_OUTPUT_TOKENS: u32 = (1u32 << MAX_OUTPUT_BITS) - 1;
/// Maximum input token value representable by 29 bits (`536_870_911`).
pub const MAX_INPUT_TOKENS: u32 = (1u32 << MAX_INPUT_BITS) - 1;

const _: () = assert!(MODALITY_BITS + MAX_OUTPUT_BITS + MAX_INPUT_BITS == 64);

/// Packed model metadata row.
#[bitfield(u64)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackedModelEntry {
    modalities: u8,
    #[bits(27)]
    max_output: u32,
    #[bits(29)]
    max_input: u32,
}

impl PackedModelEntry {
    /// Creates one packed model metadata row.
    #[inline]
    pub fn from_model_info(info: ModelInfo) -> Self {
        debug_assert!(info.max_output <= MAX_OUTPUT_TOKENS);
        debug_assert!(info.max_input <= MAX_INPUT_TOKENS);

        let mut packed = Self::new_without_defaults();
        packed.set_modalities(info.modalities.bits());
        packed.set_max_output(info.max_output);
        packed.set_max_input(info.max_input);
        packed
    }

    /// Converts a packed row into model metadata (without sampling config).
    #[inline]
    pub fn into_model_info(self) -> ModelInfo {
        ModelInfo {
            modalities: Modality::from_bits_retain(self.modalities()),
            max_input: self.max_input(),
            max_output: self.max_output(),
            temperature: None,
            top_p: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_model_entry_is_8_bytes() {
        assert_eq!(core::mem::size_of::<PackedModelEntry>(), 8);
    }

    #[test]
    fn model_entry_roundtrip() {
        let packed = PackedModelEntry::from_model_info(ModelInfo {
            modalities: Modality::TEXT | Modality::IMAGE_INPUT,
            max_output: 123_456,
            max_input: 654_321,
            temperature: None,
            top_p: None,
        });

        assert_eq!(
            packed.modalities(),
            (Modality::TEXT | Modality::IMAGE_INPUT).bits()
        );
        assert_eq!(packed.max_output(), 123_456);
        assert_eq!(packed.max_input(), 654_321);

        let unpacked = packed.into_model_info();
        assert_eq!(unpacked.modalities, Modality::TEXT | Modality::IMAGE_INPUT);
        assert_eq!(unpacked.max_output, 123_456);
        assert_eq!(unpacked.max_input, 654_321);
    }
}
