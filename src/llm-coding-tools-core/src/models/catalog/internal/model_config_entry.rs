//! Model sampling configuration entry.
//!
//! Layout (`u32`):
//! - `16` bits: temperature fixed4 (with `u16::MAX` as `None` sentinel)
//! - `16` bits: top_p fixed4 (with `u16::MAX` as `None` sentinel)

use super::Fixed4;

/// Model-configuration sidecar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct ModelConfigEntry {
    temperature: Fixed4,
    top_p: Fixed4,
}

impl ModelConfigEntry {
    /// Creates a packed row from optional sampling values.
    #[inline]
    pub fn from_sampling(temperature: Option<f32>, top_p: Option<f32>) -> Self {
        Self {
            temperature: match temperature.and_then(Fixed4::from_f32) {
                Some(f) => f,
                None => Fixed4::from_encoded(Fixed4::NONE_SENTINEL),
            },
            top_p: match top_p.and_then(Fixed4::from_f32) {
                Some(f) => f,
                None => Fixed4::from_encoded(Fixed4::NONE_SENTINEL),
            },
        }
    }

    /// Returns true when both fields are the `None` sentinel.
    #[inline]
    pub const fn is_none(self) -> bool {
        self.temperature.is_sentinel() && self.top_p.is_sentinel()
    }

    /// Returns temperature as `Option<f32>`.
    #[inline]
    pub fn temperature(self) -> Option<f32> {
        self.temperature.value()
    }

    /// Returns top_p as `Option<f32>`.
    #[inline]
    pub fn top_p(self) -> Option<f32> {
        self.top_p.value()
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
        let packed = ModelConfigEntry::from_sampling(None, None);
        assert!(packed.is_none());
        assert_eq!(packed.temperature(), None);
        assert_eq!(packed.top_p(), None);
    }

    #[test]
    fn values_roundtrip() {
        let packed = ModelConfigEntry::from_sampling(Some(1.2), Some(0.5));

        assert_eq!(packed.temperature(), Some(1.2));
        assert_eq!(packed.top_p(), Some(0.5));
    }

    #[test]
    fn partial_values() {
        let packed = ModelConfigEntry::from_sampling(Some(1.0), None);
        assert!(!packed.is_none());
        assert_eq!(packed.temperature(), Some(1.0));
        assert_eq!(packed.top_p(), None);
    }
}
