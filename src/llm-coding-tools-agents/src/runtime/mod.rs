//! Build agents with tools and default settings.
//!
//! This module holds everything you need to prepare agents for use:
//! loaded agent definitions, default settings, and available tools.
//!
//! # Public API
//!
//! Runtime construction:
//! - [`AgentRuntime`] - Your agents plus their default settings and tools
//! - [`AgentRuntimeBuilder`] - Builds an [`AgentRuntime`]
//! - [`AgentDefaults`] - Default model, temperature, and top-p when agents don't specify them
//! - [`TaskSettings`] - Shared Task delegation limits for all integrations using the runtime
//!
//! Tools:
//! - [`ToolCatalogEntry`] - One tool the runtime can provide to agents
//! - [`ToolCatalogKind`] - Which tools are available
//! - [`default_tools()`] - The standard tool set (read, write, edit, glob, grep, bash, webfetch, todo, task)
//!
//! Task delegation:
//! - [`summarize_callable_targets()`] - Builds target summaries with names and descriptions
//! - [`callable_targets()`] - Returns the agents the active agent may delegate to
//! - [`TaskTargetSummary`] - Metadata for a callable Task target
//!
//! Model resolution:
//! - [`ResolvedModel`] - A model identifier that's been validated against your catalog
//! - [`resolve_model_with_catalog()`] - Picks which model an agent will use
//! - [`ModelResolutionError`] - Errors when model selection fails
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_agents::{AgentCatalog, AgentDefaults, AgentRuntimeBuilder};
//!
//! let runtime = AgentRuntimeBuilder::new()
//!     .catalog(AgentCatalog::new())
//!     .defaults(AgentDefaults::with_model("openai/gpt-4o"))
//!     .build();
//!
//! assert!(runtime.catalog().iter().count() == 0);
//! ```

mod builder;
mod model;
mod state;
mod task;
mod tool_catalog;

pub use builder::AgentRuntimeBuilder;
pub use llm_coding_tools_core::TaskSettings;
pub use model::{resolve_model_with_catalog, ModelResolutionError, ResolvedModel};
pub use state::{AgentDefaults, AgentRuntime};
pub use task::{callable_targets, summarize_callable_targets, TaskTargetSummary};
pub use tool_catalog::{default_tools, ToolCatalogEntry, ToolCatalogKind};
