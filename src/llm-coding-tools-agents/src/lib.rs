#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]
#![warn(missing_docs)]

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
pub use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};
pub use loader::AgentLoader;
pub use parser::AgentParseError;
