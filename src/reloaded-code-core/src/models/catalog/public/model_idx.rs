//! Index into model configuration tables.

/// A 16-bit index into model metadata tables.
///
/// Used to reference a specific model configuration in the catalog's
/// packed model entry and model config entry tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ModelIdx(pub(crate) u16);

impl ModelIdx {
    /// Creates a new model index from a raw u16.
    #[inline]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Returns the underlying u16 value.
    #[inline]
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Converts to usize for indexing into slices.
    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl From<u16> for ModelIdx {
    #[inline]
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<ModelIdx> for u16 {
    #[inline]
    fn from(idx: ModelIdx) -> Self {
        idx.0
    }
}

use lite_strtab::impl_string_index;
impl_string_index!(ModelIdx: u16);
