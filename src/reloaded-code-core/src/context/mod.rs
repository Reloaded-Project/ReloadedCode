//! Tool-specific guidance for system prompts.
//!
//! This module defines the text and prompt variants used to tell the model how
//! to use tools. Built-in tools return [`ToolPrompt`] values. Shared notes such
//! as [`GIT_WORKFLOW`] and [`GITHUB_CLI`] can be added as extra sections.
//!
//! Built-in guidance stays short. Shared rules are only included when the
//! related tools are present.
//!
//! # Example
//!
//! ```rust
//! use reloaded_code_core::context::{PathMode, ToolContext, ToolPrompt};
//!
//! // Built-in tool guidance can use a structured prompt variant.
//! struct ReadTool;
//! // Custom tools can still provide plain static guidance.
//! struct NotesTool;
//!
//! impl ToolContext for ReadTool {
//!     fn name(&self) -> &'static str {
//!         "read"
//!     }
//!
//!     fn context(&self) -> ToolPrompt {
//!         ToolPrompt::Read {
//!             path_mode: PathMode::Absolute,
//!             line_numbers: true,
//!         }
//!     }
//! }
//!
//! impl ToolContext for NotesTool {
//!     fn name(&self) -> &'static str {
//!         "notes"
//!     }
//!
//!     fn context(&self) -> ToolPrompt {
//!         ToolPrompt::Static("Use this tool for short project notes.")
//!     }
//! }
//! ```

mod tool_prompt;

pub use tool_prompt::{PathMode, ToolPrompt};
pub(crate) use tool_prompt::{ToolPromptFacts, COMMON_RULES_HEADER, COMMON_RULES_SECTION_MAX_SIZE};

/// Git workflow context - commit creation guidance.
///
/// Supplemental context for agents using git via the `bash` tool.
/// Include via [`SystemPromptBuilder::add_context`](crate::SystemPromptBuilder::add_context).
pub const GIT_WORKFLOW: &str = include_str!("git_workflow.txt");

/// GitHub CLI context - gh command usage guidance.
///
/// Supplemental context for agents using the GitHub CLI via the `bash` tool.
/// Include via [`SystemPromptBuilder::add_context`](crate::SystemPromptBuilder::add_context).
pub const GITHUB_CLI: &str = include_str!("github_cli.txt");

/// Trait for tools that provide guidance for system prompts.
///
/// Implement this trait on tool types to let
/// [`SystemPromptBuilder`](crate::SystemPromptBuilder) include tool guidance
/// automatically.
///
/// # Example
///
/// ```rust
/// use reloaded_code_core::context::{ToolContext, ToolPrompt};
///
/// struct MyTool;
///
/// impl ToolContext for MyTool {
///     fn name(&self) -> &'static str {
///         "mytool"
///     }
///
///     fn context(&self) -> ToolPrompt {
///         ToolPrompt::Static("Instructions for using MyTool...")
///     }
/// }
/// ```
pub trait ToolContext {
    /// Returns the tool name for section headers in generated system prompt.
    ///
    /// Should be lowercase (e.g., "read", "bash", "glob").
    /// SystemPromptBuilder capitalizes this for display.
    #[must_use]
    fn name(&self) -> &'static str;

    /// Returns the guidance for this tool.
    #[must_use]
    fn context(&self) -> ToolPrompt;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_is_object_safe() {
        // Verify that Box<dyn ToolContext> can be constructed.
        // This proves the trait is object-safe (no associated constants,
        // no methods requiring Self: Sized).
        struct DummyTool;
        impl ToolContext for DummyTool {
            fn name(&self) -> &'static str {
                "dummy"
            }
            fn context(&self) -> ToolPrompt {
                ToolPrompt::Static("Dummy context")
            }
        }

        let _: Box<dyn ToolContext> = Box::new(DummyTool);
    }

    #[test]
    fn context_strings_are_not_empty() {
        assert!(
            !GIT_WORKFLOW.is_empty(),
            "GIT_WORKFLOW context should not be empty"
        );
        assert!(
            !GITHUB_CLI.is_empty(),
            "GITHUB_CLI context should not be empty"
        );
    }

    #[test]
    fn git_workflow_contains_expected_content() {
        assert!(
            GIT_WORKFLOW.contains("git commit"),
            "GIT_WORKFLOW should mention git commit"
        );
        assert!(
            GIT_WORKFLOW.contains("NEVER"),
            "GIT_WORKFLOW should contain safety rules"
        );
    }

    #[test]
    fn github_cli_contains_expected_content() {
        assert!(
            GITHUB_CLI.contains("gh "),
            "GITHUB_CLI should mention gh command"
        );
        assert!(
            GITHUB_CLI.contains("pull request"),
            "GITHUB_CLI should mention pull requests"
        );
    }
}
