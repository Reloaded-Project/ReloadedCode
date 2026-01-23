//! Subagent configuration loading from OpenCode-style markdown files.
//!
//! This crate provides:
//! - Frontmatter parsing for markdown files with YAML headers
//! - Agent configuration schema matching OpenCode conventions
//! - Directory scanning for agent configs in `agent/**/*.md` and `agents/**/*.md`
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

#![warn(missing_docs)]

mod config;
mod error;
mod frontmatter;
mod loader;

pub use config::{AgentConfig, AgentMode, PermissionAction, PermissionRule};
pub use error::AgentConfigError;
pub use frontmatter::{parse_frontmatter, preprocess_frontmatter, FrontmatterParseResult};
pub use loader::load_agents;
