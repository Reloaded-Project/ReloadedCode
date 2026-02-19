//! Tool context strings for LLM agents.
//!
//! These provide usage guidance, best practices, and behavioral instructions
//! for LLM agents when using coding tools. Context strings are sourced from
//! OpenCode's tool documentation.
//!
//! # Path-based Tools
//!
//! Tools operating on file paths have two variants:
//! - `*_ABSOLUTE`: For unrestricted filesystem access (absolute paths required)
//! - `*_ALLOWED`: For sandboxed access (paths relative to allowed directories)
//!
//! # Example
//!
//! ```rust
//! use llm_coding_tools_core::context::{BASH, READ_ABSOLUTE, READ_ALLOWED};
//!
//! // Use BASH context for the `bash` tool
//! println!("Bash guidance: {}", BASH);
//!
//! // Use appropriate read context based on path resolver
//! let sandboxed = true;
//! let read_context = if sandboxed { READ_ALLOWED } else { READ_ABSOLUTE };
//! ```

/// `bash` tool context - shell command execution guidance.
pub const BASH: &str = include_str!("bash.txt");

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

/// `todoread` tool context - reading task lists.
pub const TODO_READ: &str = include_str!("todoread.txt");

/// `todowrite` tool context - managing task lists.
pub const TODO_WRITE: &str = include_str!("todowrite.txt");

/// `webfetch` tool context - URL content retrieval.
pub const WEBFETCH: &str = include_str!("webfetch.txt");

/// `read` tool context for absolute path mode.
pub const READ_ABSOLUTE: &str = include_str!("read_absolute.txt");

/// `read` tool context for allowed/sandboxed path mode.
pub const READ_ALLOWED: &str = include_str!("read_allowed.txt");

/// `write` tool context for absolute path mode.
pub const WRITE_ABSOLUTE: &str = include_str!("write_absolute.txt");

/// `write` tool context for allowed/sandboxed path mode.
pub const WRITE_ALLOWED: &str = include_str!("write_allowed.txt");

/// `edit` tool context for absolute path mode.
pub const EDIT_ABSOLUTE: &str = include_str!("edit_absolute.txt");

/// `edit` tool context for allowed/sandboxed path mode.
pub const EDIT_ALLOWED: &str = include_str!("edit_allowed.txt");

/// `glob` tool context for absolute path mode.
pub const GLOB_ABSOLUTE: &str = include_str!("glob_absolute.txt");

/// `glob` tool context for allowed/sandboxed path mode.
pub const GLOB_ALLOWED: &str = include_str!("glob_allowed.txt");

/// `grep` tool context for absolute path mode.
pub const GREP_ABSOLUTE: &str = include_str!("grep_absolute.txt");

/// `grep` tool context for allowed/sandboxed path mode.
pub const GREP_ALLOWED: &str = include_str!("grep_allowed.txt");

/// Trait for tools that provide usage context for LLM system prompts.
///
/// Implement this trait on tool types (for frameworks like serdesAI) to enable automatic system prompt
/// generation via [`SystemPromptBuilder`](crate::SystemPromptBuilder).
///
/// # Example
///
/// ```rust
/// use llm_coding_tools_core::context::ToolContext;
///
/// struct MyTool;
///
/// impl ToolContext for MyTool {
///     const NAME: &'static str = "mytool";
///
///     fn context(&self) -> &'static str {
///         "Instructions for using MyTool..."
///     }
/// }
/// ```
pub trait ToolContext {
    /// Tool name used for section headers in generated system prompt.
    ///
    /// Should be lowercase (e.g., "read", "bash", "glob").
    /// SystemPromptBuilder capitalizes this for display.
    const NAME: &'static str;

    /// Returns the tool's context string for system prompt generation.
    ///
    /// This should return one of the context constants from this module.
    fn context(&self) -> &'static str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_strings_are_not_empty() {
        // Non-path tools
        assert!(!BASH.is_empty(), "BASH context should not be empty");
        assert!(
            !GIT_WORKFLOW.is_empty(),
            "GIT_WORKFLOW context should not be empty"
        );
        assert!(
            !GITHUB_CLI.is_empty(),
            "GITHUB_CLI context should not be empty"
        );
        assert!(
            !TODO_READ.is_empty(),
            "TODO_READ context should not be empty"
        );
        assert!(
            !TODO_WRITE.is_empty(),
            "TODO_WRITE context should not be empty"
        );
        assert!(!WEBFETCH.is_empty(), "WEBFETCH context should not be empty");

        // Path-based tools (absolute variants)
        assert!(
            !READ_ABSOLUTE.is_empty(),
            "READ_ABSOLUTE context should not be empty"
        );
        assert!(
            !WRITE_ABSOLUTE.is_empty(),
            "WRITE_ABSOLUTE context should not be empty"
        );
        assert!(
            !EDIT_ABSOLUTE.is_empty(),
            "EDIT_ABSOLUTE context should not be empty"
        );
        assert!(
            !GLOB_ABSOLUTE.is_empty(),
            "GLOB_ABSOLUTE context should not be empty"
        );
        assert!(
            !GREP_ABSOLUTE.is_empty(),
            "GREP_ABSOLUTE context should not be empty"
        );

        // Path-based tools (allowed variants)
        assert!(
            !READ_ALLOWED.is_empty(),
            "READ_ALLOWED context should not be empty"
        );
        assert!(
            !WRITE_ALLOWED.is_empty(),
            "WRITE_ALLOWED context should not be empty"
        );
        assert!(
            !EDIT_ALLOWED.is_empty(),
            "EDIT_ALLOWED context should not be empty"
        );
        assert!(
            !GLOB_ALLOWED.is_empty(),
            "GLOB_ALLOWED context should not be empty"
        );
        assert!(
            !GREP_ALLOWED.is_empty(),
            "GREP_ALLOWED context should not be empty"
        );
    }

    #[test]
    fn absolute_variants_mention_absolute_path() {
        assert!(
            READ_ABSOLUTE.contains("absolute path"),
            "READ_ABSOLUTE should mention absolute path"
        );
    }

    #[test]
    fn allowed_variants_mention_allowed_directories() {
        assert!(
            READ_ALLOWED.contains("allowed directories"),
            "READ_ALLOWED should mention allowed directories"
        );
        assert!(
            WRITE_ALLOWED.contains("allowed directories"),
            "WRITE_ALLOWED should mention allowed directories"
        );
        assert!(
            EDIT_ALLOWED.contains("allowed directories"),
            "EDIT_ALLOWED should mention allowed directories"
        );
        assert!(
            GLOB_ALLOWED.contains("allowed directories"),
            "GLOB_ALLOWED should mention allowed directories"
        );
        assert!(
            GREP_ALLOWED.contains("allowed directories"),
            "GREP_ALLOWED should mention allowed directories"
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

    #[test]
    fn bash_does_not_contain_git_workflow() {
        assert!(
            !BASH.contains("# Committing changes with git"),
            "BASH should not contain git workflow section"
        );
    }

    #[test]
    fn bash_does_not_contain_github_cli() {
        assert!(
            !BASH.contains("# Creating pull requests"),
            "BASH should not contain GitHub CLI section"
        );
    }
}
