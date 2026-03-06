//! models.dev API parsing and catalog-source mapping.
//!
//! - [`schema`] parses upstream `https://models.dev/api.json` into a minimal
//!   serde representation.
//! - [`catalog_sources`] maps parsed data into a
//!   [`llm_coding_tools_core::models::ModelCatalog`].
//!
//! Both modules intentionally keep only fields required by core catalog
//! construction so ingest stays fast and memory-bounded.

pub(crate) mod catalog_sources;
pub(crate) mod schema;
