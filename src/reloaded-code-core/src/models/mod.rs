//! Compact model catalog for high-performance provider/model lookup.

pub use catalog::{
    LookupTableKind, Modality, Model, ModelCatalog, ModelCatalogBuildError, ModelInfo, Provider,
    ProviderIdx, ProviderInfo, ProviderModelSource, ProviderSource,
};
pub use provider_type::ProviderType;

mod catalog;
mod provider_type;
