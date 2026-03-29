//! # Shared Types
//!
//! Central module for types used across loading and catalog operations.
//!
//! ## Re-exports
//! - Config types: [`AgentConfig`], [`AgentMode`], [`PermissionRule`], [`parse_model_parts`]
//! - Load errors: [`AgentLoadError`], [`AgentLoadResult`]
//! - Tool settings: [`AgentToolSettings`], [`ReadToolSettings`], [`GrepToolSettings`]

mod config;
mod error;
mod tool_settings;

pub use config::{parse_model_parts, AgentConfig, AgentMode, PermissionRule};
pub use error::{AgentLoadError, AgentLoadResult};
pub use tool_settings::{AgentToolSettings, GrepToolSettings, ReadToolSettings};

pub(crate) use config::RawFrontmatter;
