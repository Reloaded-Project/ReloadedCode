//! System prompt generation for LLM agents.
//!
//! Provides [`SystemPromptBuilder`] for tracking tools and generating formatted
//! system prompts containing tool usage context.

use crate::context::{
    ToolContext, ToolPrompt, ToolPromptFacts, COMMON_RULES_HEADER, COMMON_RULES_SECTION_MAX_SIZE,
};
use crate::path::AllowedPathResolver;

/// Entry storing a tool name and prompt renderer.
struct ContextEntry {
    name: &'static str,
    prompt: ToolPrompt,
}

/// Builder that tracks tools and generates formatted system prompts.
///
/// The environment section is always included and appears before tool listings.
///
/// # Example
///
/// ```no_run
/// use reloaded_code_core::context::{PathMode, ToolContext, ToolPrompt};
/// use reloaded_code_core::SystemPromptBuilder;
///
/// struct ReadTool;
///
/// impl ToolContext for ReadTool {
///     const NAME: &'static str = "read";
///
///     fn context(&self) -> ToolPrompt {
///         ToolPrompt::Read {
///             path_mode: PathMode::Absolute,
///             line_numbers: true,
///         }
///     }
/// }
///
/// let mut pb = SystemPromptBuilder::new()
///     .working_directory(std::env::current_dir().unwrap().display().to_string());
///
/// pb.track(ReadTool);
///
/// let _prompt = pb.build();
/// ```
///
/// # Output
///
/// The generated system prompt is Markdown. For example, with two tools:
///
/// ```text
/// # Environment
///
/// Working directory: /home/user/project
///
/// # Tool Usage Guidelines
///
/// ## `Read` Tool
/// Reads files from disk.
/// ## `Bash` Tool
/// Executes shell commands.
/// ```
#[derive(Default)]
pub struct SystemPromptBuilder {
    entries: Vec<ContextEntry>,
    working_directory: Option<String>,
    allowed_paths: Option<Vec<String>>,
    supplemental: Vec<(&'static str, &'static str)>,
    system_prompt: Option<String>,
}

impl SystemPromptBuilder {
    /// Creates a new system prompt builder.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records context and returns tool unchanged.
    ///
    /// Use this to wrap tools before registering them with your tool collection:
    /// ```no_run
    /// use reloaded_code_core::context::{PathMode, ToolContext, ToolPrompt};
    /// use reloaded_code_core::SystemPromptBuilder;
    ///
    /// struct MyTool;
    ///
    /// impl ToolContext for MyTool {
    ///     const NAME: &'static str = "read";
    ///
    ///     fn context(&self) -> ToolPrompt {
    ///         ToolPrompt::Read {
    ///             path_mode: PathMode::Absolute,
    ///             line_numbers: true,
    ///         }
    ///     }
    /// }
    ///
    /// let mut pb = SystemPromptBuilder::new();
    /// let _my_tool = pb.track(MyTool);
    /// // register _my_tool with your tool collection
    /// ```
    ///
    /// For example, if working with serdesAI:
    /// ```text
    /// let mut pb = SystemPromptBuilder::new();
    /// let agent = client
    ///     .builder()
    ///     .tool(pb.track(ReadTool::new()))
    ///     .system_prompt(&pb.build())
    ///     .build();
    /// ```
    pub fn track<T: ToolContext>(&mut self, tool: T) -> T {
        self.entries.push(ContextEntry {
            name: T::NAME,
            prompt: tool.context(),
        });
        tool
    }

    /// Adds supplemental context to the system prompt.
    ///
    /// Supplemental context appears in a separate "Supplemental Context" section
    /// after tool usage guidelines. Use this for guidance that isn't inherent
    /// to a specific tool, such as git workflows or GitHub CLI patterns.
    ///
    /// # Arguments
    ///
    /// * `name` - Section header (e.g., "Git Workflow", "GitHub CLI")
    /// * `context` - Context string content (e.g., [`GIT_WORKFLOW`](crate::context::GIT_WORKFLOW))
    ///
    /// # Examples
    ///
    /// Adding both git and GitHub CLI context:
    ///
    /// ```rust
    /// use reloaded_code_core::{SystemPromptBuilder, context};
    ///
    /// let pb = SystemPromptBuilder::new()
    ///     .add_context("Git Workflow", context::GIT_WORKFLOW)
    ///     .add_context("GitHub CLI", context::GITHUB_CLI);
    ///
    /// let prompt = pb.build();
    /// assert!(prompt.contains("# Supplemental Context"));
    /// assert!(prompt.contains("## Git Workflow"));
    /// ```
    ///
    /// Selective inclusion - adding only Git Workflow when not using GitHub features:
    ///
    /// ```rust
    /// use reloaded_code_core::{SystemPromptBuilder, context};
    ///
    /// // Only include git workflow for agents that use git but not GitHub
    /// let pb = SystemPromptBuilder::new()
    ///     .add_context("Git Workflow", context::GIT_WORKFLOW);
    ///
    /// let prompt = pb.build();
    /// assert!(prompt.contains("## Git Workflow"));
    /// assert!(!prompt.contains("## GitHub CLI"));
    /// ```
    #[inline]
    pub fn add_context(mut self, name: &'static str, context: &'static str) -> Self {
        self.supplemental.push((name, context));
        self
    }

    /// Sets a custom system prompt that appears first in the generated system prompt.
    ///
    /// The provided prompt is prepended before all other sections (environment,
    /// tools, supplemental context). User provides exactly what they want,
    /// including any markdown headers - no auto-modification is applied.
    ///
    /// # Example
    ///
    /// ```rust
    /// use reloaded_code_core::SystemPromptBuilder;
    ///
    /// let pb = SystemPromptBuilder::new()
    ///     .system_prompt("# System Instructions\n\nYou are a helpful assistant.");
    ///
    /// let prompt = pb.build();
    /// assert!(prompt.starts_with("# System Instructions"));
    /// ```
    #[inline]
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }
}

impl SystemPromptBuilder {
    /// Sets the working directory to display in the environment section.
    ///
    /// Accepts any type that can be converted to String, including:
    /// - `&str`
    /// - `String`
    /// - `PathBuf` or `&Path` (via `.display().to_string()`)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use reloaded_code_core::SystemPromptBuilder;
    ///
    /// let _pb = SystemPromptBuilder::new()
    ///     .working_directory("/home/user/project");
    ///
    /// // With runtime-computed path
    /// let _pb = SystemPromptBuilder::new()
    ///     .working_directory(std::env::current_dir().unwrap().display().to_string());
    /// ```
    #[inline]
    pub fn working_directory(mut self, path: impl Into<String>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Sets the allowed directories to display in the environment section.
    ///
    /// Takes an [`AllowedPathResolver`] reference and extracts its allowed paths
    /// for display. Paths are already canonicalized (absolute, symlinks resolved)
    /// by the resolver during construction.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use reloaded_code_core::{AllowedPathResolver, SystemPromptBuilder};
    ///
    /// let resolver = AllowedPathResolver::new(vec!["/home/user/project", "/tmp"]).unwrap();
    /// let _pb = SystemPromptBuilder::new()
    ///     .working_directory("/home/user/project")
    ///     .allowed_paths(&resolver);
    /// ```
    #[inline]
    pub fn allowed_paths(mut self, resolver: &AllowedPathResolver) -> Self {
        // AllowedPathResolver::allowed_paths() returns &[PathBuf] where paths
        // are already canonicalized (absolute, symlinks resolved) during
        // AllowedPathResolver::new() construction.
        self.allowed_paths = Some(
            resolver
                .allowed_paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
        );
        self
    }
}

impl SystemPromptBuilder {
    /// Generates the system prompt string with environment section.
    pub fn build(self) -> String {
        // Environment section size: ~50 bytes header + path length
        // "# Environment\nWorking directory: \n\n" = ~37 bytes
        const ENV_HEADER_SIZE: usize = 50;
        // "Allowed directories:\n- " per path + path length
        const ALLOWED_DIR_PER_ITEM: usize = 25;

        let system_prompt_size = self.system_prompt.as_ref().map_or(0, |p| p.len() + 2);

        let env_size = if self.working_directory.is_some() || self.allowed_paths.is_some() {
            ENV_HEADER_SIZE + self.working_directory.as_ref().map_or(0, |d| d.len())
        } else if self.system_prompt.is_some()
            || !self.entries.is_empty()
            || !self.supplemental.is_empty()
        {
            ENV_HEADER_SIZE
        } else {
            0
        };

        let allowed_size = self.allowed_paths.as_ref().map_or(0, |paths| {
            paths.iter().map(|p| p.len() + ALLOWED_DIR_PER_ITEM).sum()
        });

        let facts = ToolPromptFacts::from_prompts(self.entries.iter().map(|entry| entry.prompt));
        let common_rules_size = if facts.has_common_rules() {
            COMMON_RULES_SECTION_MAX_SIZE
        } else {
            0
        };
        let tools_size = self.entries.len() * 320 + common_rules_size;

        let supplemental_size: usize = self
            .supplemental
            .iter()
            .map(|(n, c)| c.len() + n.len() + 20)
            .sum();

        let has_tools = !self.entries.is_empty();
        let has_supplemental = !self.supplemental.is_empty();
        let has_system_prompt = self.system_prompt.is_some();
        let has_env_content = self.working_directory.is_some() || self.allowed_paths.is_some();

        let total_size =
            system_prompt_size + env_size + allowed_size + tools_size + supplemental_size + 90;
        let mut output = String::with_capacity(total_size);

        // Return empty if nothing to output
        if !has_tools && !has_supplemental && !has_system_prompt && !has_env_content {
            return String::new();
        }

        // System prompt (first)
        if let Some(ref prompt) = self.system_prompt {
            output.push_str(prompt);
            // Ensure single newline before next section
            if !prompt.ends_with('\n') {
                output.push('\n');
            }
        }

        // Environment section
        if has_env_content || has_system_prompt || has_tools || has_supplemental {
            output.push_str("# Environment\n");

            if let Some(ref dir) = self.working_directory {
                output.push_str("Working directory: ");
                output.push_str(dir);
                output.push('\n');
            }

            if let Some(ref paths) = self.allowed_paths {
                output.push_str("Allowed directories:\n");
                for path in paths {
                    output.push_str("- ");
                    output.push_str(path);
                    output.push('\n');
                }
            }
        }

        // Tool section
        if has_tools {
            output.push_str("# Tool Usage Guidelines\n");

            if facts.has_common_rules() {
                output.push_str(COMMON_RULES_HEADER);
                facts.write_common_rules(&mut output);
            }

            for entry in self.entries {
                output.push_str("## `");
                let mut chars = entry.name.chars();
                if let Some(first) = chars.next() {
                    output.push(first.to_ascii_uppercase());
                    output.push_str(chars.as_str());
                } else {
                    output.push_str(entry.name);
                }
                output.push_str("` Tool\n");
                entry.prompt.render(&mut output, facts);
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
        }

        // Supplemental context section
        if has_supplemental {
            output.push_str("# Supplemental Context\n");

            for (name, context) in self.supplemental {
                output.push_str("## ");
                output.push_str(name);
                output.push('\n');
                output.push_str(context);
                if !context.ends_with('\n') {
                    output.push('\n');
                }
            }
        }

        output.truncate(output.trim_end().len());
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::PathMode;
    use crate::tool_metadata::{bash, edit, glob, grep, read, task, write};
    use indoc::indoc;

    struct MockTool {
        id: u32,
    }

    impl ToolContext for MockTool {
        const NAME: &'static str = "mock";
        fn context(&self) -> ToolPrompt {
            ToolPrompt::Static("Mock tool context.")
        }
    }

    struct OtherTool;

    impl ToolContext for OtherTool {
        const NAME: &'static str = "other";
        fn context(&self) -> ToolPrompt {
            ToolPrompt::Static("Other context.")
        }
    }

    const fn built_in_path_mode<const ALLOWED: bool>() -> PathMode {
        if ALLOWED {
            PathMode::Allowed
        } else {
            PathMode::Absolute
        }
    }

    macro_rules! built_in_path_tool_with_line_numbers {
        ($tool:ident, $name:expr, $variant:ident) => {
            struct $tool<const ALLOWED: bool, const LINE_NUMBERS: bool>;

            impl<const ALLOWED: bool, const LINE_NUMBERS: bool> ToolContext
                for $tool<ALLOWED, LINE_NUMBERS>
            {
                const NAME: &'static str = $name;

                fn context(&self) -> ToolPrompt {
                    ToolPrompt::$variant {
                        path_mode: built_in_path_mode::<ALLOWED>(),
                        line_numbers: LINE_NUMBERS,
                    }
                }
            }
        };
    }

    macro_rules! built_in_path_tool {
        ($tool:ident, $name:expr, $variant:ident) => {
            struct $tool<const ALLOWED: bool>;

            impl<const ALLOWED: bool> ToolContext for $tool<ALLOWED> {
                const NAME: &'static str = $name;

                fn context(&self) -> ToolPrompt {
                    ToolPrompt::$variant {
                        path_mode: built_in_path_mode::<ALLOWED>(),
                    }
                }
            }
        };
    }

    macro_rules! built_in_tool {
        ($tool:ident, $name:expr, $prompt:expr) => {
            struct $tool;

            impl ToolContext for $tool {
                const NAME: &'static str = $name;

                fn context(&self) -> ToolPrompt {
                    $prompt
                }
            }
        };
    }

    built_in_path_tool_with_line_numbers!(BuiltInReadTool, read::NAME, Read);
    built_in_path_tool!(BuiltInWriteTool, write::NAME, Write);
    built_in_path_tool!(BuiltInEditTool, edit::NAME, Edit);
    built_in_path_tool!(BuiltInGlobTool, glob::NAME, Glob);
    built_in_path_tool_with_line_numbers!(BuiltInGrepTool, grep::NAME, Grep);
    built_in_tool!(
        BuiltInBashTool,
        bash::NAME,
        ToolPrompt::Bash {
            network_disabled: false,
            sandboxed: false
        }
    );
    built_in_tool!(BuiltInTaskTool, task::NAME, ToolPrompt::Task);

    fn assert_no_triple_newlines(preamble: &str) {
        assert!(
            !preamble.contains("\n\n\n"),
            "Found triple newline in preamble.\nGot:\n{preamble}"
        );
    }

    fn assert_no_trailing_whitespace(preamble: &str) {
        assert_eq!(
            preamble,
            preamble.trim_end(),
            "Preamble has trailing whitespace"
        );
    }

    #[test]
    fn empty_builder_returns_empty_string() {
        let preamble = SystemPromptBuilder::new().build();
        assert!(preamble.is_empty());
    }

    #[test]
    fn track_returns_tool_unchanged() {
        let mut pb = SystemPromptBuilder::new();
        let tool = MockTool { id: 42 };
        let returned = pb.track(tool);
        assert_eq!(returned.id, 42);
    }

    #[test]
    fn single_tool_formats_correctly() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user");
        let _ = pb.track(MockTool { id: 1 });
        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(preamble.contains("Working directory: /home/user"));
        assert!(preamble.contains("# Tool Usage Guidelines"));
        assert!(preamble.contains("## `Mock` Tool"));
        assert!(preamble.contains("Mock tool context."));
    }

    #[test]
    fn multiple_tools_preserve_order() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user");
        let _ = pb.track(MockTool { id: 1 });
        let _ = pb.track(OtherTool);
        let preamble = pb.build();

        let mock_pos = preamble.find("## `Mock` Tool").unwrap();
        let other_pos = preamble.find("## `Other` Tool").unwrap();
        assert!(
            mock_pos < other_pos,
            "Tools should appear in insertion order"
        );
    }

    #[test]
    fn multiple_tools_have_single_newline_between() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user");
        let _ = pb.track(MockTool { id: 1 });
        let _ = pb.track(OtherTool);
        let preamble = pb.build();

        // Verify exact transition: context ends, then next tool header
        assert!(
            preamble.contains("Mock tool context.\n## `Other` Tool"),
            "Expected single newline between tool sections.\nGot:\n{preamble}"
        );

        // Verify single newline after tool header
        assert!(
            preamble.contains("## `Mock` Tool\nMock tool context."),
            "Expected single newline after tool header.\nGot:\n{preamble}"
        );

        // Verify no extra blank line after Environment header
        assert!(
            preamble.contains("# Environment\nWorking directory:"),
            "Expected single newline after Environment header.\nGot:\n{preamble}"
        );

        // Verify no extra blank line after section header
        assert!(
            preamble.contains("# Tool Usage Guidelines\n## `Mock` Tool"),
            "Expected single newline after section header.\nGot:\n{preamble}"
        );

        assert_no_trailing_whitespace(&preamble);
    }

    #[test]
    fn no_blank_lines_between_major_sections() {
        let mut pb = SystemPromptBuilder::new()
            .working_directory("/home/user/project")
            .add_context("Git Workflow", "Git guidance.");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        // Verify no blank line between Environment and Tool Usage Guidelines
        assert!(
            preamble.contains("Working directory: /home/user/project\n# Tool Usage Guidelines"),
            "Expected single newline before Tool Usage Guidelines.\nGot:\n{preamble}"
        );

        // Verify no blank line between Tool Usage Guidelines and Supplemental Context
        assert!(
            preamble.contains("Mock tool context.\n# Supplemental Context"),
            "Expected single newline before Supplemental Context.\nGot:\n{preamble}"
        );

        // Verify no blank lines immediately after section headers
        assert!(
            preamble.contains("# Environment\nWorking"),
            "Expected no blank line after # Environment.\nGot:\n{preamble}"
        );
        assert!(
            preamble.contains("# Tool Usage Guidelines\n##"),
            "Expected no blank line after # Tool Usage Guidelines.\nGot:\n{preamble}"
        );
        assert!(
            preamble.contains("# Supplemental Context\n##"),
            "Expected no blank line after # Supplemental Context.\nGot:\n{preamble}"
        );
        assert_no_triple_newlines(&preamble);
    }

    #[test]
    fn empty_environment_section_has_single_newline_boundaries() {
        let mut pb = SystemPromptBuilder::new().add_context("Git Workflow", "Git guidance.");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        assert!(
            preamble.contains("# Environment\n# Tool Usage Guidelines"),
            "Expected Tool Usage Guidelines immediately after empty Environment section.\nGot:\n{preamble}"
        );
        assert!(
            preamble.contains("Mock tool context.\n# Supplemental Context"),
            "Expected Supplemental Context immediately after tool content.\nGot:\n{preamble}"
        );
        assert_no_triple_newlines(&preamble);
    }

    #[test]
    fn builder_includes_environment_section() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let _ = pb.track(MockTool { id: 1 });
        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(preamble.contains("Working directory: /home/user/project"));
        // Environment should come before tools
        let env_pos = preamble.find("# Environment").unwrap();
        let tools_pos = preamble.find("# Tool Usage Guidelines").unwrap();
        assert!(env_pos < tools_pos);
    }

    #[test]
    fn builder_with_working_dir_only_renders_environment_section() {
        // Environment section should render even without tools tracked
        let pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(preamble.contains("Working directory: /home/user/project"));
        assert!(!preamble.contains("Allowed directories:"));
        assert!(!preamble.contains("# Tool Usage Guidelines"));
    }

    #[test]
    fn working_directory_accepts_runtime_string() {
        // Simulates std::env::current_dir().unwrap().display().to_string()
        let runtime_path = String::from("/runtime/computed/path");
        let preamble = SystemPromptBuilder::new()
            .working_directory(runtime_path)
            .build();

        assert!(preamble.contains("Working directory: /runtime/computed/path"));
    }

    #[test]
    fn working_directory_accepts_str() {
        let preamble = SystemPromptBuilder::new()
            .working_directory("/static/path")
            .build();

        assert!(preamble.contains("Working directory: /static/path"));
    }

    #[test]
    fn builder_with_allowed_paths_shows_paths() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new(vec![dir.path()]).unwrap();

        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .allowed_paths(&resolver);
        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(preamble.contains("Working directory: /home/user"));
        assert!(preamble.contains("Allowed directories:"));
        // Check that the temp dir path appears (canonicalized)
        assert!(preamble.contains(&dir.path().canonicalize().unwrap().display().to_string()));
    }

    #[test]
    fn builder_with_only_allowed_paths_no_working_dir() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new(vec![dir.path()]).unwrap();

        let pb = SystemPromptBuilder::new().allowed_paths(&resolver);
        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(!preamble.contains("Working directory:"));
        assert!(preamble.contains("Allowed directories:"));
    }

    #[test]
    fn allowed_paths_format_is_bulleted_absolute_paths() {
        use std::path::Path;
        use tempfile::TempDir;

        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new(vec![dir1.path(), dir2.path()]).unwrap();

        let pb = SystemPromptBuilder::new().allowed_paths(&resolver);
        let preamble = pb.build();

        // Check format: "- <absolute_path>" (cross-platform)
        let lines: Vec<&str> = preamble.lines().collect();
        let allowed_idx = lines
            .iter()
            .position(|l| l.contains("Allowed directories"))
            .unwrap();

        for i in 1..=2 {
            let line = lines[allowed_idx + i];
            assert!(
                line.starts_with("- "),
                "Line should start with '- ': {}",
                line
            );
            let path_str = line.strip_prefix("- ").unwrap();
            assert!(
                Path::new(path_str).is_absolute(),
                "Path should be absolute: {}",
                path_str
            );
        }
    }

    #[test]
    fn allowed_paths_appears_after_working_directory() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let resolver = AllowedPathResolver::new(vec![dir.path()]).unwrap();

        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .allowed_paths(&resolver);
        let preamble = pb.build();

        let working_dir_pos = preamble.find("Working directory:").unwrap();
        let allowed_pos = preamble.find("Allowed directories:").unwrap();
        assert!(
            working_dir_pos < allowed_pos,
            "Working directory should appear before allowed paths"
        );
    }

    #[test]
    fn add_context_without_tools_renders_environment_before_supplemental() {
        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", "Git guidance content.");

        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(!preamble.contains("# Tool Usage Guidelines"));
        assert!(preamble.contains("# Supplemental Context"));
        assert!(preamble.contains("## Git Workflow"));
        assert!(preamble.contains("Git guidance content."));
        let supplemental_pos = preamble.find("# Supplemental Context").unwrap();
        assert!(preamble.find("# Environment").unwrap() < supplemental_pos);
    }

    #[test]
    fn add_context_multiple_sections_preserve_order() {
        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", "Git content.")
            .add_context("GitHub CLI", "GitHub content.");

        let preamble = pb.build();

        let git_pos = preamble.find("## Git Workflow").unwrap();
        let github_pos = preamble.find("## GitHub CLI").unwrap();
        assert!(
            git_pos < github_pos,
            "Contexts should appear in insertion order"
        );
    }

    #[test]
    fn add_context_with_env_and_tools() {
        let mut pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", "Git guidance.");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        let env_pos = preamble.find("# Environment").unwrap();
        let tools_pos = preamble.find("# Tool Usage Guidelines").unwrap();
        let supplemental_pos = preamble.find("# Supplemental Context").unwrap();

        assert!(env_pos < tools_pos);
        assert!(tools_pos < supplemental_pos);
    }

    #[test]
    fn add_context_no_triple_newlines() {
        let mut pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", "Git guidance.\n");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        assert_no_triple_newlines(&preamble);
    }

    #[test]
    fn add_context_with_actual_git_workflow_constant() {
        use crate::context::GIT_WORKFLOW;

        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", GIT_WORKFLOW);

        let preamble = pb.build();

        assert!(preamble.contains("# Supplemental Context"));
        assert!(preamble.contains("## Git Workflow"));
        // Verify actual content from git_workflow.txt is included
        assert!(
            preamble.contains("Only create commits when requested"),
            "Should contain git commit workflow content"
        );
        assert!(
            preamble.contains("Git Safety Protocol"),
            "Should contain safety protocol section"
        );
    }

    #[test]
    fn add_context_with_actual_github_cli_constant() {
        use crate::context::GITHUB_CLI;

        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("GitHub CLI", GITHUB_CLI);

        let preamble = pb.build();

        assert!(preamble.contains("# Supplemental Context"));
        assert!(preamble.contains("## GitHub CLI"));
        // Verify actual content from github_cli.txt is included
        assert!(
            preamble.contains("gh pr create"),
            "Should contain gh pr create example"
        );
    }

    #[test]
    fn add_context_selective_inclusion_git_only() {
        use crate::context::{GITHUB_CLI, GIT_WORKFLOW};

        // Only include git workflow (not GitHub CLI)
        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", GIT_WORKFLOW);

        let preamble = pb.build();

        assert!(preamble.contains("## Git Workflow"));
        assert!(!preamble.contains("## GitHub CLI"));
        assert!(!preamble.contains(GITHUB_CLI));
    }

    #[test]
    fn add_context_both_git_and_github() {
        use crate::context::{GITHUB_CLI, GIT_WORKFLOW};

        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user")
            .add_context("Git Workflow", GIT_WORKFLOW)
            .add_context("GitHub CLI", GITHUB_CLI);

        let preamble = pb.build();

        assert!(preamble.contains("## Git Workflow"));
        assert!(preamble.contains("## GitHub CLI"));
        // Verify order preserved
        let git_pos = preamble.find("## Git Workflow").unwrap();
        let github_pos = preamble.find("## GitHub CLI").unwrap();
        assert!(
            git_pos < github_pos,
            "Git Workflow should appear before GitHub CLI"
        );
    }

    #[test]
    fn system_prompt_appears_first() {
        let pb = SystemPromptBuilder::new()
            .system_prompt("# System Instructions\n\nYou are a helpful assistant.")
            .working_directory("/home/user");

        let preamble = pb.build();

        assert!(
            preamble.starts_with("# System Instructions"),
            "System prompt should appear first.\nGot:\n{preamble}"
        );

        let system_pos = preamble.find("# System Instructions").unwrap();
        let env_pos = preamble.find("# Environment").unwrap();
        assert!(
            system_pos < env_pos,
            "System prompt should appear before environment section"
        );
    }

    #[test]
    fn system_prompt_appears_before_tools() {
        let mut pb =
            SystemPromptBuilder::new().system_prompt("# Custom Header\n\nMy custom instructions.");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        let system_pos = preamble.find("# Custom Header").unwrap();
        let tools_pos = preamble.find("# Tool Usage Guidelines").unwrap();
        assert!(
            system_pos < tools_pos,
            "System prompt should appear before tools section"
        );
    }

    #[test]
    fn system_prompt_no_modification() {
        // User provides exact content, no auto-header added
        let custom = "My custom content without header";
        let pb = SystemPromptBuilder::new().system_prompt(custom);

        let preamble = pb.build();

        assert!(
            preamble.starts_with("My custom content without header"),
            "System prompt should not be modified.\nGot:\n{preamble}"
        );
    }

    #[test]
    fn system_prompt_optional_default_behavior() {
        // Without system_prompt, existing behavior preserved
        let mut pb = SystemPromptBuilder::new();
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        assert!(
            preamble.starts_with("# Environment"),
            "Without system prompt, should start with Environment.\nGot:\n{preamble}"
        );
    }

    #[test]
    fn system_prompt_only_produces_output() {
        let pb = SystemPromptBuilder::new()
            .system_prompt("# Just Instructions\n\nOnly system prompt, no tools.");

        let preamble = pb.build();

        assert!(!preamble.is_empty());
        assert!(preamble.contains("# Just Instructions"));
        assert!(!preamble.contains("# Tool Usage Guidelines"));
    }

    #[test]
    fn system_prompt_with_env_and_tools_and_supplemental() {
        let mut pb = SystemPromptBuilder::new()
            .system_prompt("# System\n\nInstructions.")
            .working_directory("/home/user")
            .add_context("Git Workflow", "Git guidance.");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        let system_pos = preamble.find("# System").unwrap();
        let env_pos = preamble.find("# Environment").unwrap();
        let tools_pos = preamble.find("# Tool Usage Guidelines").unwrap();
        let supplemental_pos = preamble.find("# Supplemental Context").unwrap();

        assert!(system_pos < env_pos);
        assert!(env_pos < tools_pos);
        assert!(tools_pos < supplemental_pos);
    }

    #[test]
    fn system_prompt_no_trailing_newline_gets_separator() {
        // System prompt without trailing newline should get "\n" separator
        let mut pb = SystemPromptBuilder::new().system_prompt(indoc! {
            "# System

            No trailing newline"
        });
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        // Should have exactly one newline between system prompt and environment
        assert!(
            preamble.contains("No trailing newline\n# Environment"),
            "Expected single newline after system prompt.\nGot:\n{preamble}"
        );
        assert_no_triple_newlines(&preamble);
    }

    #[test]
    fn system_prompt_single_trailing_newline_no_extra() {
        // System prompt ending with \n needs no extra separator
        let pb = SystemPromptBuilder::new()
            .system_prompt(indoc! {"
                # System

                Ends with single newline
            "})
            .working_directory("/home/user");

        let preamble = pb.build();

        // Should have exactly one newline between system prompt and environment
        assert!(
            preamble.contains("Ends with single newline\n# Environment"),
            "Expected single newline after system prompt.\nGot:\n{preamble}"
        );
        assert_no_triple_newlines(&preamble);
    }

    #[test]
    fn system_prompt_preserves_trailing_newlines() {
        // System prompt with trailing blank lines are preserved as-is;
        // build() only adds a newline when none exists.
        let mut pb = SystemPromptBuilder::new().system_prompt("# System\n\nContent.\n\n");
        let _ = pb.track(MockTool { id: 1 });

        let preamble = pb.build();

        // build() should not add extra newlines beyond what the user provided
        assert!(
            preamble.contains("Content.\n\n# Environment"),
            "build() should not add newlines to already-newline-terminated content.\nGot:\n{preamble}"
        );
    }

    #[test]
    fn preamble_preview_structure_has_correct_section_order() {
        // Mirrors the example binary to verify structure
        let resolver = AllowedPathResolver::from_canonical(["/home/user/project", "/tmp"]);

        let mut pb = SystemPromptBuilder::new()
            .system_prompt(indoc! {"
                # System Instructions

                You are helpful."})
            .working_directory("/home/user/project")
            .allowed_paths(&resolver)
            .add_context("Git Workflow", "Git guidance content.")
            .add_context("GitHub CLI", "GitHub guidance content.");

        let _ = pb.track(MockTool { id: 1 });
        let _ = pb.track(OtherTool);

        let preamble = pb.build();

        // Verify all sections present
        assert!(
            preamble.contains("# System Instructions"),
            "Missing system prompt"
        );
        assert!(
            preamble.contains("# Environment"),
            "Missing environment section"
        );
        assert!(
            preamble.contains("Working directory:"),
            "Missing working directory"
        );
        assert!(
            preamble.contains("Allowed directories:"),
            "Missing allowed directories"
        );
        assert!(
            preamble.contains("# Tool Usage Guidelines"),
            "Missing tools section"
        );
        assert!(
            preamble.contains("# Supplemental Context"),
            "Missing supplemental section"
        );

        // Verify section order: system -> env -> tools -> supplemental
        let system_pos = preamble.find("# System Instructions").unwrap();
        let env_pos = preamble.find("# Environment").unwrap();
        let tools_pos = preamble.find("# Tool Usage Guidelines").unwrap();
        let supplemental_pos = preamble.find("# Supplemental Context").unwrap();

        assert!(
            system_pos < env_pos,
            "System prompt should come before environment"
        );
        assert!(env_pos < tools_pos, "Environment should come before tools");
        assert!(
            tools_pos < supplemental_pos,
            "Tools should come before supplemental"
        );

        // Verify no formatting issues
        assert_no_triple_newlines(&preamble);
        assert_no_trailing_whitespace(&preamble);
    }

    #[test]
    fn preamble_preview_allowed_paths_rendered_correctly() {
        let resolver = AllowedPathResolver::from_canonical(["/home/user/project", "/tmp"]);

        let pb = SystemPromptBuilder::new()
            .working_directory("/home/user/project")
            .allowed_paths(&resolver);

        let preamble = pb.build();

        // Verify both paths appear as bullet points
        assert!(
            preamble.contains("- /home/user/project"),
            "Missing project path"
        );
        assert!(preamble.contains("- /tmp"), "Missing tmp path");
    }

    #[test]
    fn built_in_tools_emit_common_rules_once() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let _ = pb.track(BuiltInReadTool::<true, true>);
        let _ = pb.track(BuiltInWriteTool::<true>);
        let _ = pb.track(BuiltInEditTool::<true>);
        let _ = pb.track(BuiltInBashTool);
        let _ = pb.track(BuiltInGlobTool::<true>);
        let _ = pb.track(BuiltInGrepTool::<true, true>);

        let preamble = pb.build();

        assert!(preamble.contains("## Common Rules"));
        assert_eq!(
            preamble
                .matches("Only listed allowed directories may be accessed")
                .count(),
            1
        );
        assert!(preamble.contains("Prefer `glob`, `grep`, `read`, `edit`, and `write` over `bash`"));
        assert!(preamble.contains(
            "Prefer `edit` for targeted changes and `write` for new files or full rewrites."
        ));
        assert!(preamble.contains("copy exact text from `read` and omit any `{n}: ` prefixes"));
    }

    #[test]
    fn built_in_tools_omit_unavailable_tool_references() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let _ = pb.track(BuiltInReadTool::<false, false>);
        let _ = pb.track(BuiltInEditTool::<false>);

        let preamble = pb.build();

        assert!(preamble.contains("## Common Rules"));
        assert!(preamble.contains("Read a file before `edit`, then copy exact text from `read`."));
        assert!(preamble.contains("- Returns raw text. Lines over `2000` chars are truncated."));
        assert!(preamble.contains("- Reads files, not directories."));
        assert!(!preamble.contains("`glob`"));
        assert!(!preamble.contains("`bash`"));
        assert!(!preamble.contains("`write`"));
        assert!(!preamble.contains("L{n}: "));
    }

    #[test]
    fn task_rule_lists_only_available_local_tools() {
        let mut pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let _ = pb.track(BuiltInReadTool::<false, true>);
        let _ = pb.track(BuiltInTaskTool);

        let preamble = pb.build();

        assert!(preamble.contains("Do not use it when `read` on one or a few files is enough."));
        assert!(!preamble.contains("`glob` on one or a few files is enough"));
        assert!(!preamble.contains("`grep` on one or a few files is enough"));
    }

    #[test]
    fn bash_section_conditional_lines() {
        struct SandboxedBashTool;

        impl ToolContext for SandboxedBashTool {
            const NAME: &'static str = bash::NAME;
            fn context(&self) -> ToolPrompt {
                ToolPrompt::Bash {
                    network_disabled: true,
                    sandboxed: true,
                }
            }
        }

        struct HostBashTool;

        impl ToolContext for HostBashTool {
            const NAME: &'static str = bash::NAME;
            fn context(&self) -> ToolPrompt {
                ToolPrompt::Bash {
                    network_disabled: false,
                    sandboxed: false,
                }
            }
        }

        let mut pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let _ = pb.track(SandboxedBashTool);
        let sandboxed = pb.build();

        let mut pb = SystemPromptBuilder::new().working_directory("/home/user/project");
        let _ = pb.track(HostBashTool);
        let host = pb.build();

        // Sandboxed: both conditional lines present, network line exactly once.
        let network_lines: Vec<_> = sandboxed
            .lines()
            .filter(|l| l.contains("Network access is disabled in this sandbox."))
            .collect();
        assert_eq!(network_lines.len(), 1);
        assert!(sandboxed.contains("Commands run inside a Linux sandbox."));

        // Host: neither conditional line present.
        assert!(!host.contains("Network access is disabled"));
        assert!(!host.contains("Commands run inside a Linux sandbox"));
    }
}
