//! Compact model catalog for high-performance provider/model lookup.

mod catalog;
mod provider_type;

pub use catalog::{
    LookupTableKind, Modality, Model, ModelCatalog, ModelCatalogBuildError, ModelInfo, Provider,
    ProviderInfo, ProviderModelSource, ProviderSource,
};
pub use provider_type::ProviderType;
