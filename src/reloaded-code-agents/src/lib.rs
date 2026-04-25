#![doc = include_str!(concat!("../", env!("CARGO_PKG_README")))]

mod catalog;
mod extensions;
mod loader;
mod parser;
mod path;
mod runtime;
mod types;

pub use catalog::AgentCatalog;
pub use extensions::RulesetExt;
pub use loader::AgentLoader;
pub use parser::AgentParseError;
pub use path::{build_resolver_for_tool, FileToolResolver};
pub use runtime::{
    callable_targets, default_tools, resolve_model_with_catalog, summarize_callable_targets,
    AgentDefaults, AgentRuntime, AgentRuntimeBuilder, ModelResolutionError, ResolvedModel,
    TaskSettings, TaskTargetSummary, ToolCatalogEntry, ToolCatalogKind,
};
pub use types::{
    parse_model_parts, AgentConfig, AgentLoadError, AgentLoadResult, AgentMode, AgentToolSettings,
    BashToolSettings, GlobToolSettings, GrepToolSettings, PermissionRule, ReadToolSettings,
    WebFetchToolSettings,
};
