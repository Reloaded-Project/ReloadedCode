//! Model sampling configuration entry.
//!
//! Layout (`u32`):
//! - `16` bits: temperature fixed4 (with `u16::MAX` as `None` sentinel)
//! - `16` bits: top_p fixed4 (with `u16::MAX` as `None` sentinel)

use super::Fixed4;
use crate::models::catalog::public::builder_types::ModelConfig;

/// Model-configuration sidecar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct ModelConfigEntry {
    temperature: Fixed4,
    top_p: Fixed4,
}

impl ModelConfigEntry {
    /// Creates a packed row from optional public model config.
    #[inline]
    pub fn from_model_config(config: Option<ModelConfig>) -> Self {
        match config {
            Some(config) => Self {
                temperature: config.temperature,
                top_p: config.top_p,
            },
            None => Self {
                temperature: Fixed4::from_encoded(Fixed4::NONE_SENTINEL),
                top_p: Fixed4::from_encoded(Fixed4::NONE_SENTINEL),
            },
        }
    }

    /// Returns true when both fields are the `None` sentinel.
    #[inline]
    pub const fn is_none(self) -> bool {
        self.temperature.is_sentinel() && self.top_p.is_sentinel()
    }

    /// Converts a packed row into optional public model config.
    #[inline]
    pub fn into_model_config(self) -> Option<ModelConfig> {
        if self.temperature.is_sentinel() && self.top_p.is_sentinel() {
            None
        } else {
            Some(ModelConfig {
                temperature: self.temperature,
                top_p: self.top_p,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_config_entry_is_4_bytes() {
        assert_eq!(core::mem::size_of::<ModelConfigEntry>(), 4);
    }

    #[test]
    fn none_roundtrips() {
        let packed = ModelConfigEntry::from_model_config(None);
        assert!(packed.is_none());
        assert_eq!(packed.into_model_config(), None);
    }

    #[test]
    fn values_roundtrip() {
        let packed = ModelConfigEntry::from_model_config(Some(ModelConfig {
            temperature: Fixed4::from_encoded(12_000),
            top_p: Fixed4::from_encoded(5_000),
        }));

        let unpacked = packed.into_model_config().expect("config must exist");
        assert!(!unpacked.temperature.is_sentinel());
        assert_eq!(unpacked.temperature.encoded(), 12_000);
        assert!(!unpacked.top_p.is_sentinel());
        assert_eq!(unpacked.top_p.encoded(), 5_000);
    }
}
