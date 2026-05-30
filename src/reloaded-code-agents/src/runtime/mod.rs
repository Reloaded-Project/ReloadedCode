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
//! # use reloaded_code_agents::{AgentCatalog, AgentDefaults, AgentRuntimeBuilder};
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let runtime = AgentRuntimeBuilder::new()
//!     .catalog(AgentCatalog::new())
//!     .defaults(AgentDefaults::with_model("openai/gpt-5.4"))
//!     .build()?;
//!
//! assert!(runtime.catalog().iter().count() == 0);
//! # Ok(())
//! # }
//! ```

mod builder;
mod model;
mod state;
mod task;

pub use builder::AgentRuntimeBuilder;
pub use model::{resolve_model_with_catalog, ModelResolutionError, ResolvedModel};
pub use reloaded_code_core::TaskSettings;
pub use state::{AgentDefaults, AgentRuntime};
pub use task::{callable_targets, summarize_callable_targets, TaskTargetSummary};
