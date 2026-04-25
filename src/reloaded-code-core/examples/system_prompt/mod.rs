#![allow(dead_code)]
#![allow(unused_imports)]

//! Shared helpers for system prompt preview examples.
//!
//! These helpers build example prompts and matching tool definitions so the
//! example binaries can compare static request cost.
//!
//! Not production ready, just for testing/demonstration.
//!
//! # Public API
//! - [`build_case`] renders one example scenario.
//! - [`estimate_tokens`] approximates token count from character count.
//! - [`print_footprint`] prints the prompt and tool-definition totals.
//! - [`print_ranked_sizes`] prints a sorted size breakdown.
//! - [`section_sizes`] ranks rendered guideline sections by size.
//! - [`PromptCase`] and related config types describe one example scenario.

mod build;
mod definitions;
mod mock_tools;
mod report;
mod types;

pub use build::build_case;
pub use report::{
    estimate_tokens, print_footprint, print_ranked_sizes, print_tool_definitions, section_sizes,
};
pub use types::{GrepConfig, PromptArtifacts, PromptCase, ReadConfig, TaskTarget};

fn sort_sizes_desc(sizes: &mut [(String, usize)]) {
    sizes.sort_unstable_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
}
