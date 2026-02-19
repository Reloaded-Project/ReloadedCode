//! # Shared Types
//!
//! Central module for types used across loading and catalog operations.
//!
//! ## Re-exports
//! - Config types: [`AgentConfig`], [`AgentMode`], [`PermissionRule`]
//! - Load errors: [`AgentLoadError`], [`AgentLoadResult`]

mod config;
mod error;

pub use config::{AgentConfig, AgentMode, PermissionRule};
pub use error::{AgentLoadError, AgentLoadResult};

pub(crate) use config::RawFrontmatter;
