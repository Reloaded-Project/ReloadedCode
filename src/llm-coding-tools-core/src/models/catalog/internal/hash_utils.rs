//! Internal hash utilities for the model catalog.

use crate::internal::hash64::Hash64;
use ahash::RandomState;
use core::hash::{BuildHasher, Hasher};

#[inline(always)]
pub fn provider_table_entry_hash(entry: &super::PackedProviderTableEntry) -> u64 {
    entry.hash48()
}

#[inline(always)]
pub fn provider_model_table_entry_hash(entry: &super::PackedProviderModelTableEntry) -> u64 {
    entry.hash48()
}

#[inline(always)]
pub fn hash_provider_key(hash_state: &RandomState, provider_key: &str) -> Hash64 {
    Hash64::from_u64(hash_state.hash_one(provider_key.as_bytes()))
}

#[inline(always)]
pub fn hash_provider_model_key(
    hash_state: &RandomState,
    provider_key: &str,
    model_key: &str,
) -> Hash64 {
    let mut hasher = hash_state.build_hasher();
    hasher.write(provider_key.as_bytes());
    hasher.write_u8(0xFF);
    hasher.write(model_key.as_bytes());
    Hash64::from_u64(hasher.finish())
}

#[inline(always)]
pub fn hash_state_for_seed(seed: u8) -> RandomState {
    // Using ahash's generate_with() creates an independent hash function
    // by mixing the seed with internal entropy. Each call produces a
    // different RandomState even with the same seed value.
    RandomState::generate_with(u64::from(seed), 0, 0, 0)
}
