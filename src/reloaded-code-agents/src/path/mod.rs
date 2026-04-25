//! File-tool path resolver construction.
//!
//! Re-exports [`FileToolResolver`] and [`build_resolver_for_tool`] from the
//! `resolver` submodule. See that module for optimisation-tier details.

mod resolver;

pub use resolver::{build_resolver_for_tool, FileToolResolver};
