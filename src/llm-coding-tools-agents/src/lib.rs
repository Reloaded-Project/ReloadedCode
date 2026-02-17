#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

mod catalog;
mod config;
mod error;
mod extensions;
mod loader;
mod parser;

pub use catalog::AgentCatalog;
pub use config::{AgentConfig, AgentMode, PermissionRule};
pub use error::AgentLoadError;
pub use error::AgentLoadResult;
pub use extensions::RulesetExt;
pub use loader::AgentLoader;
pub use parser::AgentParseError;
