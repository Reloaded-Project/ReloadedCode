#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

mod catalog;
mod extensions;
mod loader;
mod parser;
mod types;

pub use catalog::AgentCatalog;
pub use extensions::RulesetExt;
pub use loader::AgentLoader;
pub use parser::AgentParseError;
pub use types::{AgentConfig, AgentLoadError, AgentLoadResult, AgentMode, PermissionRule};
