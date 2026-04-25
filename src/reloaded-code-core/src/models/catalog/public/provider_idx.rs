//! Index into provider tables.

/// A 16-bit index into provider metadata tables.
///
/// Used to reference a specific provider in the catalog's
/// packed provider entry tables and string tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, bitcode::Encode, bitcode::Decode)]
#[repr(transparent)]
pub struct ProviderIdx(pub(crate) u16);

impl ProviderIdx {
    /// Creates a new provider index from a raw u16.
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

impl From<u16> for ProviderIdx {
    #[inline]
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<ProviderIdx> for u16 {
    #[inline]
    fn from(idx: ProviderIdx) -> Self {
        idx.0
    }
}

use lite_strtab::impl_string_index;
impl_string_index!(ProviderIdx: u16);
