//! Model sampling configuration entry.
//!
//! Layout (`u32`):
//! - `16` bits: temperature fixed4 (with `u16::MAX` as `None` sentinel)
//! - `16` bits: top_p fixed4 (with `u16::MAX` as `None` sentinel)

use super::Fixed4;
use crate::models::catalog::ModelCatalogBuildError;

/// Model-configuration sidecar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct ModelConfigEntry {
    temperature: Fixed4,
    top_p: Fixed4,
}

impl ModelConfigEntry {
    /// Creates a packed row from optional sampling values.
    ///
    /// Returns an error if either `temperature` or `top_p` is `Some` with an
    /// invalid value (negative or exceeds `Fixed4::MAX_ENCODED`).
    #[inline]
    pub fn from_sampling(
        temperature: Option<f32>,
        top_p: Option<f32>,
    ) -> Result<Self, ModelCatalogBuildError> {
        let temperature = match temperature {
            None => Fixed4::from_encoded(Fixed4::NONE_SENTINEL),
            Some(v) => match Fixed4::from_f32(v) {
                Some(f) => f,
                None => {
                    return Err(ModelCatalogBuildError::InvalidSamplingValue {
                        field: "temperature",
                        value: v,
                    });
                }
            },
        };
        let top_p = match top_p {
            None => Fixed4::from_encoded(Fixed4::NONE_SENTINEL),
            Some(v) => match Fixed4::from_f32(v) {
                Some(f) => f,
                None => {
                    return Err(ModelCatalogBuildError::InvalidSamplingValue {
                        field: "top_p",
                        value: v,
                    });
                }
            },
        };
        Ok(Self { temperature, top_p })
    }

    /// Returns true when both fields are the `None` sentinel.
    #[inline]
    pub const fn is_none(self) -> bool {
        self.temperature.is_sentinel() && self.top_p.is_sentinel()
    }

    /// Returns the raw temperature fixed4 value (may be sentinel).
    #[inline]
    pub(crate) const fn temperature_fixed(self) -> Fixed4 {
        self.temperature
    }

    /// Returns the raw top_p fixed4 value (may be sentinel).
    #[inline]
    pub(crate) const fn top_p_fixed(self) -> Fixed4 {
        self.top_p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::catalog::ModelCatalogBuildError;

    #[test]
    fn model_config_entry_is_4_bytes() {
        assert_eq!(core::mem::size_of::<ModelConfigEntry>(), 4);
    }

    #[test]
    fn none_roundtrips() {
        let packed = ModelConfigEntry::from_sampling(None, None).unwrap();
        assert!(packed.is_none());
        assert_eq!(packed.temperature_fixed().value(), None);
        assert_eq!(packed.top_p_fixed().value(), None);
    }

    #[test]
    fn values_roundtrip() {
        let packed = ModelConfigEntry::from_sampling(Some(1.2), Some(0.5)).unwrap();

        assert_eq!(packed.temperature_fixed().value(), Some(1.2));
        assert_eq!(packed.top_p_fixed().value(), Some(0.5));
    }

    #[test]
    fn partial_values() {
        let packed = ModelConfigEntry::from_sampling(Some(1.0), None).unwrap();
        assert!(!packed.is_none());
        assert_eq!(packed.temperature_fixed().value(), Some(1.0));
        assert_eq!(packed.top_p_fixed().value(), None);
    }

    #[test]
    fn invalid_temperature_returns_error() {
        let result = ModelConfigEntry::from_sampling(Some(-0.1), None);
        assert!(matches!(
            result,
            Err(ModelCatalogBuildError::InvalidSamplingValue {
                field: "temperature",
                value: -0.1,
            })
        ));
    }

    #[test]
    fn invalid_top_p_returns_error() {
        let result = ModelConfigEntry::from_sampling(None, Some(10.0));
        assert!(matches!(
            result,
            Err(ModelCatalogBuildError::InvalidSamplingValue {
                field: "top_p",
                value: 10.0,
            })
        ));
    }
}
