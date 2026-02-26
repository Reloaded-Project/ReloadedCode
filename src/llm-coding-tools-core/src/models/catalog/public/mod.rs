//! Public types for the model catalog.
//!
//! This module contains public-facing data types used when building
//! and querying a [`ModelCatalog`].

pub use builder_types::{LookupTableKind, ModelCatalogBuildError, ModelInfo, ProviderInfo};
pub use entry::{CatalogEntry, Model, Provider};
pub use modality::Modality;
pub use model_idx::ModelIdx;
pub use provider_idx::ProviderIdx;

pub(crate) mod builder_types;
mod entry;
mod modality;
mod model_idx;
mod provider_idx;
