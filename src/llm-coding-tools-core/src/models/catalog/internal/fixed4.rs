//! Fixed4 decimal type for sampling values.
//!
//! Fixed4 means values are stored as `value * 10_000` in a `u16`.
//! For example, `1.0` is encoded as `10_000`.
//!
//! The value `u16::MAX` is reserved as a `None` sentinel for packed optional
//! fields. This type is optional-aware - instances can represent valid values
//! or the sentinel (None).

/// Fixed-point decimal with 4 fractional digits.
///
/// Encoded as `value * 10_000` in a `u16`. The value `u16::MAX` is reserved
/// as a `None` sentinel for packed optional fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Fixed4(u16);

impl Fixed4 {
    /// Scale factor (`10_000 => 1.0000`).
    pub const SCALE: u16 = 10_000;
    /// Sentinel representing `None` in packed optional fields.
    pub const NONE_SENTINEL: u16 = u16::MAX;
    /// Maximum encodable value (excludes the `None` sentinel).
    pub const MAX_ENCODED: u16 = u16::MAX - 1;

    /// Creates from an encoded fixed4 value (including sentinel).
    #[inline]
    pub const fn from_encoded(encoded: u16) -> Self {
        Self(encoded)
    }

    /// Creates from an encoded fixed4 value, returning `None` if sentinel.
    #[inline]
    pub const fn from_encoded_checked(encoded: u16) -> Option<Self> {
        if encoded == Self::NONE_SENTINEL {
            None
        } else {
            Some(Self(encoded))
        }
    }

    /// Returns the raw encoded fixed4 representation (may be sentinel).
    #[inline]
    pub const fn encoded(self) -> u16 {
        self.0
    }

    /// Returns `true` if this instance represents the `None` sentinel.
    #[inline]
    pub const fn is_sentinel(self) -> bool {
        self.0 == Self::NONE_SENTINEL
    }

    /// Returns `true` if this instance represents a valid fixed4 value.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != Self::NONE_SENTINEL
    }

    /// Converts to `Option<Fixed4>`, returning `None` if sentinel.
    #[inline]
    pub const fn to_option(self) -> Option<Self> {
        if self.0 == Self::NONE_SENTINEL {
            None
        } else {
            Some(self)
        }
    }

    /// Returns the user-facing floating-point value, or `None` if sentinel.
    #[inline]
    pub fn value(self) -> Option<f32> {
        if self.0 == Self::NONE_SENTINEL {
            None
        } else {
            Some(f32::from(self.0) / f32::from(Self::SCALE))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_detection() {
        let sentinel = Fixed4::from_encoded(Fixed4::NONE_SENTINEL);
        assert!(sentinel.is_sentinel());
        assert!(!sentinel.is_valid());
        assert_eq!(sentinel.to_option(), None);
        assert_eq!(sentinel.value(), None);

        let valid = Fixed4::from_encoded(10_000);
        assert!(!valid.is_sentinel());
        assert!(valid.is_valid());
        assert_eq!(valid.to_option(), Some(valid));
    }

    #[test]
    fn value_roundtrips() {
        let fixed4 = Fixed4::from_encoded(12_000);
        assert_eq!(fixed4.encoded(), 12_000);
        assert_eq!(fixed4.value(), Some(1.2_f32));
    }

    #[test]
    fn from_encoded_checked() {
        let valid = Fixed4::from_encoded_checked(5_000);
        assert_eq!(valid.map(|f| f.encoded()), Some(5_000));

        let sentinel = Fixed4::from_encoded_checked(Fixed4::NONE_SENTINEL);
        assert_eq!(sentinel, None);
    }

    #[test]
    fn max_encoded_excludes_sentinel() {
        assert_eq!(Fixed4::MAX_ENCODED, Fixed4::NONE_SENTINEL - 1);
    }
}
