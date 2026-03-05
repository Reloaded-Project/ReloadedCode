//! Compact model catalog for high-performance provider/model lookup.

mod catalog;
mod provider_type;

pub use catalog::{
    CatalogEntry, LookupTableKind, Modality, Model, ModelCatalog, ModelCatalogBuildError,
    ModelInfo, Provider, ProviderInfo, ProviderModelSourceRow, ProviderSourceRow,
};
pub use provider_type::ProviderType;
