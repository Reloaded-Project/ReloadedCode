//! Internal implementation details for the model catalog.
//!
//! This module contains implementation details that are not part of the public API.
//! Items here may change without notice.

pub(crate) use builder::build_from_source;
pub use fixed4::Fixed4;

// Re-export hash utilities
pub use hash_utils::{
    hash_provider_key, hash_provider_model_key, hash_state_for_seed,
    provider_model_table_entry_hash, provider_table_entry_hash,
};

// Re-export constants needed by the main catalog
pub use packed_env_range::{MAX_ENV_RANGE_COUNT, MAX_ENV_START};
pub use packed_model_entry::{MAX_INPUT_TOKENS, MAX_OUTPUT_TOKENS};
pub use packed_provider_model_table_entry::MAX_MODEL_CONFIG_COUNT;
pub use packed_provider_table_entry::MAX_PROVIDER_COUNT;

mod builder;
mod fixed4;
mod hash_utils;
mod model_config_entry;
mod packed_env_range;
mod packed_model_entry;
mod packed_provider_model_table_entry;
mod packed_provider_table_entry;

// Re-export internal types for use by the main catalog module
pub use model_config_entry::ModelConfigEntry;
pub use packed_env_range::PackedEnvRange;
pub use packed_model_entry::PackedModelEntry;
pub use packed_provider_model_table_entry::PackedProviderModelTableEntry;
pub use packed_provider_table_entry::PackedProviderTableEntry;
