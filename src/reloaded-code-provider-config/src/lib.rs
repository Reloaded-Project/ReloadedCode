//! YAML-based custom provider configuration.
//!
//! Parse provider definitions from YAML files, merge multiple sources,
//! and convert them into catalog types for [`ModelCatalog::build()`].
//!
//! [`ModelCatalog::build()`]: reloaded_code_core::models::ModelCatalog::build

pub use config::{ModelConfig, ProviderConfig};
pub use error::ProviderConfigError;
pub use loader::{default_config_paths, LoadedProviderConfig, ProviderConfigLoader};

mod api_type;
mod config;
mod error;
mod loader;
