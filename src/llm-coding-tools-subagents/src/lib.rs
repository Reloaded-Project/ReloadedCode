//! Subagent configuration loading and permission management.
//!
//! This crate provides:
//! - Frontmatter parsing for markdown files with YAML headers
//! - Agent configuration schema matching OpenCode conventions
//! - Directory scanning for agent configs in `agent/**/*.md` and `agents/**/*.md`
//! - Permission evaluation with wildcard pattern matching (last-match-wins)
//! - Subagent registry with mode filtering and permission-aware access control
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_subagents::{load_agents, AgentConfig};
//! use std::path::Path;
//!
//! let agents = load_agents(&[Path::new("~/.opencode")]).unwrap();
//! for (name, config) in &agents {
//!     println!("{}: {}", name, config.description);
//! }
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
mod frontmatter;
mod loader;
mod permission;
mod registry;

pub use config::{AgentConfig, AgentMode, PermissionAction, PermissionRule};
pub use error::AgentConfigError;
pub use frontmatter::{parse_frontmatter, preprocess_frontmatter, FrontmatterParseResult};
pub use loader::load_agents;
pub use permission::{Rule, Ruleset};
pub use registry::SubagentRegistry;
