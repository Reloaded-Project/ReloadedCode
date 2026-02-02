//! Agent configuration loading and permission management.
//!
//! This crate provides:
//! - Config-only [`AgentCatalog`] for loading and iterating agent configs
//! - Directory scanning for agent configs in `agent/**/*.md` and `agents/**/*.md`
//! - Permission evaluation with wildcard pattern matching (last-match-wins)
//! - [`AgentLoader`] for composing agent configs from multiple sources
//! - [`TaskInput`] / [`TaskOutput`] types for framework Task tools
//!
//! The new registry-driven Task flow:
//! 1. Load agent configs into [`AgentCatalog`] using [`AgentLoader`]
//! 2. Build a framework-specific registry (e.g., SerdesAI `AgentRegistryBuilder`)
//! 3. Construct `TaskTool` from the registry and permission rules
//!
//! # Example: Load agents
//!
//! ```no_run
//! use llm_coding_tools_agents::{AgentLoader, AgentCatalog};
//! use std::path::Path;
//!
//! let mut loader = AgentLoader::new();
//! let mut catalog = AgentCatalog::new();
//! loader.add_directory(&mut catalog, Path::new("/etc/opencode"))?;
//! loader.add_file(&mut catalog, Path::new("/path/to/custom_agent.md"))?;
//! # Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
//! ```
//!
//! # Example: Complete Task tool setup
//!
//! See the framework-specific READMEs for complete examples:
//!
//! - **SerdesAI**: See `llm-coding-tools-serdesai` README for Task tool setup
//!
//! The flow:
//! 1. Load agent configs into [`AgentCatalog`] using [`AgentLoader`]
//! 2. Build a framework-specific registry (e.g., SerdesAI `AgentRegistryBuilder`)
//! 3. Construct `TaskTool` from the registry and permission rules
//!
//! See `examples/serdesai-agents.rs` for a complete runnable example.
//!
//! # Permission System
//!
//! Permissions use a ruleset with allow/deny actions and wildcard patterns.
//! Evaluation follows a last-match-wins policy with default deny.
//!
//! ```
//! use llm_coding_tools_agents::{Ruleset, Rule, PermissionAction};
//!
//! let mut ruleset = Ruleset::new();
//! ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
//! ruleset.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));
//!
//! assert!(ruleset.is_allowed("task", "orchestrator-builder"));
//! assert!(!ruleset.is_allowed("task", "random-agent"));
//! ```

#![warn(missing_docs)]

mod catalog;
mod config;
mod error;
mod loader;
mod parser;
mod permission;
mod task;

pub use catalog::AgentCatalog;
pub use config::{AgentConfig, AgentMode, PermissionAction, PermissionRule};
pub use error::AgentLoadError;
pub use error::AgentLoadResult;
pub use loader::AgentLoader;
pub use parser::AgentParseError;
pub use permission::{Rule, Ruleset};
pub use task::{TaskInput, TaskOutput};
