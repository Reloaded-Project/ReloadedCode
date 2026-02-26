//! Efficient catalog/registry of providers and models sourced
//! from places like 'models.dev'. Contains bare minimum of information
//! required for usage.
//!
//! For instance; a model entry like `synthetic/hf:moonshotai/Kimi-K2.5` may be
//! split into:
//!
//! Provider: 'synthetic'
//! Model: 'hf:moonshotai/Kimi-K2.5'
//!
//! Internally the `provider`(s) and `model`(s) are stored in separate tables;
//! with friendly APIs to return those back combined when needed.
//!
//! # Public API
//!
//! ## Building a Catalog
//!
//! - [`ModelCatalogBuilder`] - Incremental builder for constructing a catalog
//! - [`ModelInfo`] - Model metadata input (modalities, token limits, sampling)
//! - [`ProviderInfo`] - Provider metadata input (API URL, env vars, type)
//! - [`Modality`] - Content modality flags (text, image, audio, video)
//!
//! ## Querying a Catalog
//!
//! - [`ModelCatalog`] - Immutable lookup catalog
//! - [`Model`] - Model lookup result
//! - [`Provider`] - Provider lookup result
//! - [`CatalogEntry`] - Combined provider + model lookup result
//!
//! ## Error Handling
//!
//! - [`ModelCatalogBuildError`] - Errors during catalog construction
//! - [`LookupTableKind`] - Identifies which hash table had a collision
//!
//! # Why split provider and model tables?
//!
//! Many providers share the same models. Although they may sometimes be renamed,
//! e.g. `Kimi-K2.5` vs `hf:moonshotai/Kimi-K2.5`; they often have identical
//! metadata. (token limits, modalities, etc.)
//!
//! Given a snapshot of models.dev from 20th of February 2026, we have:
//!
//! - Unique model IDs: 1,669
//! - Unique model configurations: 552
//!
//! We optimize for this case hashtables of `hash -> index` 😉
//!
//! # Memory Optimizations
//!
//! To save on memory, we don't actually store the original strings for provider
//! or model anywhere. The typical use case is that a user has a given provider &
//! model ID e.g. `synthetic/hf:moonshotai/Kimi-K2.5` and just needs to pull
//! up metadata for it. e.g. when `model` is specified in an agent file.
//!
//! Instead, we provide a guarantee that a *VALID* user provided provider and
//! model key will always hash to unique values (0 collisions). Since the
//! `ModelCatalog` is usually constructed once at startup, this is something
//! we can practically guarantee. (negligible failure probability)
//!
//! Sometimes this concept is referred to as a 'perfect hash', elsewhere.
//!
//! ## Hash Collision Probabilities
//!
//! Currently `ProviderTable` and `ModelTable` use 48 bits for the hash.
//!
//! | Odds of collision | # 48-bit hash values |
//! | ----------------- | -------------------: |
//! | 1 in 2            |           19,753,663 |
//! | 1 in 10           |            7,701,474 |
//! | 1 in 100          |            2,378,621 |
//! | 1 in 1,000        |              750,488 |
//! | 1 in 10,000       |              237,272 |
//! | 1 in 100,000      |               75,031 |
//! | 1 in 1 million    |               23,727 |
//! | 1 in 10 million   |                7,503 |
//! | 1 in 100 million  |                2,373 |
//! | 1 in 1 billion    |                  751 |
//! | 1 in 10 billion   |                  238 |
//! | 1 in 100 billion  |                   76 |
//! | 1 in 1 trillion   |                   24 |
//! | 1 in 10 trillion  |                    8 |
//!
//! Today's probabilities of 'at least 1 collision' are:
//!
//! - `ProviderTable`: 96 entries, 48-bit hash -> about `1 in 61 billion`
//! - `ModelTable`: 1,669 entries, 48-bit hash -> about `1 in 202 million`
//!
//! Note: Above assumes a 'perfect' hash function with uniformly distributed output.
//!       While such function does not exist in practice, 'ahash' which I used
//!       here has very good distribution and comes fairly close.
//!
//! ## Reseeding
//!
//! As an additional safety measure, re-seeding is also supported.
//! i.e. Using alternative seeds for hashing.
//!
//! ProviderTable (96 entries, 48-bit):
//!
//! | Seeds | Odds of failure      |
//! | ----- | -------------------: |
//! | 1     | 1 in 62 billion      |
//! | 2     | 1 in 3.8 quintillion |
//! | 4     | 1 in 1.4 x 10^43     |
//! | 8     | 1 in 2.1 x 10^86     |
//! | 16    | 1 in 4.4 x 10^172    |
//!
//! ModelTable (1,669 entries, 48-bit):
//!
//! | Seeds | Odds of failure      |
//! | ----- | -------------------: |
//! | 1     | 1 in 202 million     |
//! | 2     | 1 in 41 quadrillion  |
//! | 4     | 1 in 1.7 x 10^33     |
//! | 8     | 1 in 2.8 x 10^66     |
//! | 16    | 1 in 7.8 x 10^132    |
//!
//! This basically seals the deal, ensuring a collision will never happen.
//!
//! As a point of reference, there are estimated to be 10^78 to 10^82 atoms in
//! the observable universe.
//!
//! # Numeric Limits
//!
//! | Limit                     |       Value | Description                                      |
//! | ------------------------- | ----------: | ------------------------------------------------ |
//! | Max providers             |      65,536 | Addressable by 16-bit provider index             |
//! | Max model configs         |      65,536 | Addressable by 16-bit model configuration index  |
//! | Max provider env vars     |      16,384 | Per-provider env-var pool offset (14-bit)        |
//! | Max env vars per provider |           3 | Count field in provider entry (2-bit)            |
//! | Max input tokens          | 536,870,911 | 29-bit packed field (≈536M)                      |
//! | Max output tokens         | 134,217,727 | 27-bit packed field (≈134M)                      |
//! | Hash bits retained        |          48 | Truncated from 64-bit ahash output               |
//! | Max reseed attempts       |          16 | Number of alternative hash seeds                 |
//!
//! Note: There's technically 16 bits per provider, but only 14 bits for provider env var.
//! Since each provider typically has 1 env var; that means 14 bits for provider, effectively.
//!
//! # Detailed Memory Layout
//!
//! This layout is optimized for scenarios where many providers host overlapping
//! models. Numbers below are from real API data (`api.json`):
//!
//! ## Statistics
//!
//! | Metric                               | Value   |
//! | ------------------------------------ | ------: |
//! | Unique providers                     |      96 |
//! | Total model entries                  |   3,031 |
//! | Unique model configurations          |     585 |
//! | Avg models sharing same config       |    5.18 |
//!
//! ## Packed Metadata Storage
//!
//! | Field                 | Type                                  | Size | Count |   Total  |
//! | --------------------- | ------------------------------------- | ---- | ----- | -------: |
//! | `provider_table`      | `HashTable<PackedProviderTableEntry>` | 8 B  |    96 |    768 B |
//! | `model_table`         | `HashTable<PackedModelTableEntry>`    | 8 B  | 3,031 | 24,248 B |
//! | `provider_entries`    | `Box<[ProviderType]>`                 | 1 B  |    96 |     96 B |
//! | `model_entries`       | `Box<[PackedModelEntry]>`             | 8 B  |   585 |  4,680 B |
//! | `provider_env_ranges` | `Box<[PackedEnvRange]>`               | 2 B  |    96 |    192 B |
//!
//! **Packed metadata total: ~26.0 KB**
//!
//! ## Optional Metadata
//!
//! The `model_config_entries` field stores preset sampling parameters (`temperature`,
//! `top_p`) as [`ModelConfigEntry`] (4 bytes each). models.dev does not provide
//! this so this is currently markes as `None`.
//!
//! | Field                  | Type                               | Size | Count | Total |
//! | ---------------------- | ---------------------------------- | ---- | ----- | ----: |
//! | `model_config_entries` | `Option<Box<[ModelConfigEntry]>>`  | 4 B  |     0 |    —  |
//!
//! ## String Table Storage
//!
//! | Field               | Type                           | String Data | Offsets |   Total  |
//! | ------------------- | ------------------------------ | ----------: | ------: | -------: |
//! | `provider_api_urls` | `StringTable<u32, ProviderIdx>`|    2,460 B  |   296 B |  2,756 B |
//! | `provider_env_keys` | `StringTable<u32, ProviderIdx>`|    1,904 B  |   436 B |  2,340 B |
//!
//! **String tables total: ~5.1 KB** (null-terminated strings + 4-byte offsets)
//!
//! ## Other Runtime State
//!
//! | Field        | Type          | Size |
//! | ------------ | ------------- | ---- |
//! | `hash_state` | `RandomState` | ~8 B |
//!
//! String tables use `lite_strtab` with 4-byte offsets.
//!
//! ## Deduplication
//!
//! The key insight is that `ModelTable` keys can point to shared
//! `ModelEntry` / `ModelConfigEntry` rows. When multiple providers host the
//! same model, we only store the metadata once. This is why we have 1,669
//! model keys but only 552 unique model configurations.

pub use builder::ModelCatalogBuilder;
pub use catalog::ModelCatalog;
pub use public::*;

mod builder;
#[allow(clippy::module_inception)]
mod catalog;
mod internal;
mod public;
