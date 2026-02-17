//! Bit-packed permission storage.
//!
//! Packs permission hash and action into a single u64.
//! - Lower 63 bits: hash63 (upper 63 bits of original hash)
//! - Upper 1 bit (bit 63): PermissionAction

use crate::internal::hash63::Hash63;
use crate::internal::hash64::Hash64;
use crate::permissions::PermissionAction;

/// Action bit mask - highest bit (bit 63).
const ACTION_MASK: u64 = 1u64 << 63;

/// Hash mask - lower 63 bits.
const HASH_MASK: u64 = !ACTION_MASK;

// Compile-time assertion: PermissionAction must be 1 byte for bit-packing
const _: () = assert!(
    std::mem::size_of::<PermissionAction>() == 1,
    "PermissionAction must be 1 byte for bit-packing"
);

/// A u64 containing both permission hash and action.
///
/// Layout:
/// - Bits 0-62: hash63 (upper 63 bits of original hash)
/// - Bit 63: PermissionAction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PackedPermission(u64);

impl PackedPermission {
    /// Creates a packed permission from hash and action.
    #[inline]
    pub(crate) fn new(hash: Hash64, action: PermissionAction) -> Self {
        let hash63 = Hash63::from_hash64(hash);
        let action_bit = (action as u64) << 63;
        Self(hash63.as_u64() | action_bit)
    }

    /// Returns the hash portion (lower 63 bits) as a [`Hash63`].
    /// Use `Hash63::from_hash64()` to compare with an original Hash64.
    #[inline]
    pub(crate) fn hash(&self) -> Hash63 {
        Hash63::from_u64(self.0 & HASH_MASK)
    }

    /// Returns the PermissionAction stored in bit 63.
    #[inline]
    pub(crate) fn action(&self) -> PermissionAction {
        unsafe { std::mem::transmute(((self.0 >> 63) & 1) as u8) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::hash63::Hash63;
    use crate::internal::hash64::Hash64;

    #[test]
    fn pack_unpack_roundtrip() {
        // Use distinctive pattern: 0x1122334455667788 (easily detect if bits are lost)
        let hash = Hash64::from_u64(0x1122334455667788u64);
        let hash_shifted = Hash63::from_hash64(hash);

        // Test roundtrip for Allow
        let packed_allow = PackedPermission::new(hash, PermissionAction::Allow);
        assert_eq!(packed_allow.hash(), hash_shifted);
        assert_eq!(packed_allow.action(), PermissionAction::Allow);

        // Test roundtrip for Deny
        let packed_deny = PackedPermission::new(hash, PermissionAction::Deny);
        assert_eq!(packed_deny.hash(), hash_shifted);
        assert_eq!(packed_deny.action(), PermissionAction::Deny);
    }
}
