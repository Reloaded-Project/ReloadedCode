//! Subagent configuration loading and permission management.
//!
//! This crate provides:
//! - Agent configuration schema matching OpenCode conventions
//! - Directory scanning for agent configs in `agent/**/*.md` and `agents/**/*.md`
//! - Permission evaluation with wildcard pattern matching (last-match-wins)
//! - Subagent registry with mode filtering and permission-aware access control
//! - Flexible agent loading via [`AgentLoader`] for composing sources
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_subagents::{AgentLoader, SubagentRegistry};
//! use std::path::Path;
//!
//! let mut loader = AgentLoader::new();
//! let mut registry = SubagentRegistry::new();
//! loader.add_directory(&mut registry, Path::new("/etc/opencode"))?;
//! loader.add_file(&mut registry, Path::new("/path/to/custom_agent.md"))?;
//! # Ok::<(), llm_coding_tools_subagents::AgentLoadError>(())
//! ```
//!
//! # Permission System
//!
//! Permissions use a ruleset with allow/deny actions and wildcard patterns.
//! Evaluation follows a last-match-wins policy with default deny.
//!
//! ```
//! use llm_coding_tools_subagents::{Ruleset, Rule, PermissionAction};
//!
//! let mut ruleset = Ruleset::new();
//! ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
//! ruleset.push(Rule::new("task", "orchestrator-*", PermissionAction::Allow));
//!
//! assert!(ruleset.is_allowed("task", "orchestrator-builder"));
//! assert!(!ruleset.is_allowed("task", "random-agent"));
//! ```

#![warn(missing_docs)]

mod config;
mod error;
mod loader;
mod parser;
mod permission;
mod registry;
mod task;

pub use config::{AgentConfig, AgentMode, PermissionAction, PermissionRule};
pub use error::AgentLoadError;
pub use error::AgentLoadResult;
pub use loader::AgentLoader;
pub use parser::AgentParseError;
pub use permission::{Rule, Ruleset};
pub use registry::SubagentRegistry;
pub use task::{TaskError, TaskInput, TaskOutput, TaskRunner, TaskToolCore};
