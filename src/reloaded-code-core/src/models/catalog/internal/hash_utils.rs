//! Internal hash utilities for the model catalog.

use crate::internal::hash64::Hash64;
use ahash::RandomState;
use core::hash::{BuildHasher, Hasher};

/// Hashes a provider key into a [`Hash64`] using the given hash state.
///
/// # Arguments
///
/// - `hash_state`: Hash state used to derive the hash function.
/// - `provider_key`: Raw provider key string to hash.
#[inline(always)]
pub fn hash_provider_key(hash_state: &RandomState, provider_key: &str) -> Hash64 {
    Hash64::from_u64(hash_state.hash_one(provider_key.as_bytes()))
}

/// Hashes a provider key and model key pair into a [`Hash64`].
///
/// The two keys are written into the hasher with a `0xFF` separator byte
/// to prevent ambiguous collisions across concatenation boundaries.
///
/// # Arguments
///
/// - `hash_state`: Hash state used to build the hasher.
/// - `provider_key`: Provider key written first into the hash.
/// - `model_key`: Model key written after the separator byte.
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

/// Creates an independent [`RandomState`] derived from a seed.
///
/// Using ahash's `generate_with` mixes the seed with internal entropy, so
/// each call produces a different hash function even for the same seed.
///
/// # Arguments
///
/// - `seed`: Seed value mixed into the generated hash state.
#[inline(always)]
pub fn hash_state_for_seed(seed: u8) -> RandomState {
    // Using ahash's generate_with() creates an independent hash function
    // by mixing the seed with internal entropy. Each call produces a
    // different RandomState even with the same seed value.
    RandomState::generate_with(u64::from(seed), 0, 0, 0)
}

/// Returns the truncated 48-bit hash stored in a packed provider-model table entry.
///
/// # Arguments
///
/// - `entry`: Packed provider-model table entry whose stored hash is returned.
#[inline(always)]
pub fn provider_model_table_entry_hash(entry: &super::PackedProviderModelTableEntry) -> u64 {
    entry.hash48()
}

/// Returns the truncated 48-bit hash stored in a packed provider table entry.
///
/// # Arguments
///
/// - `entry`: Packed provider table entry whose stored hash is returned.
#[inline(always)]
pub fn provider_table_entry_hash(entry: &super::PackedProviderTableEntry) -> u64 {
    entry.hash48()
}
