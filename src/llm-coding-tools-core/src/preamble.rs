//! Preamble generation for LLM agents.
//!
//! Provides [`PreambleBuilder`] for tracking tools and generating formatted
//! preambles containing tool usage context.

use crate::context::ToolContext;

/// Entry storing tool name and context string.
struct ContextEntry {
    name: &'static str,
    context: &'static str,
}

/// Builder that tracks tools and generates formatted preambles.
///
/// # Generic Parameters
///
/// - `ENV`: When `true`, includes an environment section with working directory
///   before tool listings. Defaults to `false` for backwards compatibility.
///
/// # Example
///
/// ```no_run
/// use llm_coding_tools_core::context::{ToolContext, READ_ABSOLUTE};
/// use llm_coding_tools_core::PreambleBuilder;
///
/// struct ReadTool;
///
/// impl ToolContext for ReadTool {
///     const NAME: &'static str = "read";
///
///     fn context(&self) -> &'static str {
///         READ_ABSOLUTE
///     }
/// }
///
/// // Without environment section (default)
/// let mut pb = PreambleBuilder::<false>::new();
/// let _preamble = pb.build();
///
/// // With environment section
/// let mut pb = PreambleBuilder::<true>::new()
///     .working_directory(std::env::current_dir().unwrap().display().to_string());
///
/// pb.track(ReadTool);
///
/// let _preamble = pb.build();
/// ```
///
/// # Output
///
/// The generated preamble is Markdown. For example, with two tools:
///
/// ```text
/// # Tool Usage Guidelines
///
/// ## Read Tool
///
/// Reads files from disk.
///
/// ## Bash Tool
///
/// Executes shell commands.
/// ```
///
/// When the environment section is enabled and a working directory is provided:
///
/// ```text
/// # Environment
///
/// Working directory: /home/user/project
///
/// # Tool Usage Guidelines
///
/// ## Read Tool
///
/// Reads files from disk.
/// ```
pub struct PreambleBuilder<const ENV: bool = false> {
    entries: Vec<ContextEntry>,
    working_directory: Option<String>,
}

impl<const ENV: bool> Default for PreambleBuilder<ENV> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            working_directory: None,
        }
    }
}

impl<const ENV: bool> PreambleBuilder<ENV> {
    /// Creates a new preamble builder.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records context and returns tool unchanged.
    ///
    /// Use this to wrap tools before registering them with your tool collection:
    /// ```no_run
    /// use llm_coding_tools_core::context::{ToolContext, READ_ABSOLUTE};
    /// use llm_coding_tools_core::PreambleBuilder;
    ///
    /// struct MyTool;
    ///
    /// impl ToolContext for MyTool {
    ///     const NAME: &'static str = "read";
    ///
    ///     fn context(&self) -> &'static str {
    ///         READ_ABSOLUTE
    ///     }
    /// }
    ///
    /// let mut pb = PreambleBuilder::<false>::new();
    /// let _my_tool = pb.track(MyTool);
    /// // register _my_tool with your tool collection
    /// ```
    ///
    /// For example, if working with rig's ToolSet builder:
    /// ```ignore
    /// let mut pb = PreambleBuilder::new();
    /// let toolset = ToolSet::builder()
    ///     .static_tool(pb.track(ReadTool::new()))
    ///     .build();
    /// ```
    pub fn track<T: ToolContext>(&mut self, tool: T) -> T {
        self.entries.push(ContextEntry {
            name: T::NAME,
            context: tool.context(),
        });
        tool
    }
}

impl PreambleBuilder<true> {
    /// Sets the working directory to display in the environment section.
    ///
    /// Accepts any type that can be converted to String, including:
    /// - `&str`
    /// - `String`
    /// - `PathBuf` or `&Path` (via `.display().to_string()`)
    ///
    /// Only available when environment section is enabled (`PreambleBuilder<true>`).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use llm_coding_tools_core::PreambleBuilder;
    ///
    /// let _pb = PreambleBuilder::<true>::new()
    ///     .working_directory("/home/user/project");
    ///
    /// // With runtime-computed path
    /// let _pb = PreambleBuilder::<true>::new()
    ///     .working_directory(std::env::current_dir().unwrap().display().to_string());
    /// ```
    #[inline]
    pub fn working_directory(mut self, path: impl Into<String>) -> Self {
        self.working_directory = Some(path.into());
        self
    }
}

impl PreambleBuilder<false> {
    /// Generates the preamble string without environment section.
    pub fn build(self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }

        let tools_size: usize = self
            .entries
            .iter()
            .map(|e| e.context.len() + e.name.len() + 20)
            .sum();

        let mut output = String::with_capacity(tools_size + 30);

        output.push_str("# Tool Usage Guidelines\n\n");

        for entry in self.entries {
            output.push_str("## ");
            let mut chars = entry.name.chars();
            if let Some(first) = chars.next() {
                output.push(first.to_ascii_uppercase());
                output.push_str(chars.as_str());
            }
            output.push_str(" Tool\n\n");
            output.push_str(entry.context);
            output.push_str("\n\n");
        }

        output.truncate(output.trim_end().len());
        output
    }
}

impl PreambleBuilder<true> {
    /// Generates the preamble string with environment section.
    pub fn build(self) -> String {
        // Environment section size: ~50 bytes header + path length
        // "# Environment\n\nWorking directory: \n\n" = ~38 bytes
        const ENV_HEADER_SIZE: usize = 50;

        let env_size = self
            .working_directory
            .as_ref()
            .map_or(0, |d| d.len() + ENV_HEADER_SIZE);

        let tools_size: usize = self
            .entries
            .iter()
            .map(|e| e.context.len() + e.name.len() + 20)
            .sum();

        let has_tools = !self.entries.is_empty();
        let has_env = self.working_directory.is_some();

        // Return empty if nothing to output
        if !has_tools && !has_env {
            return String::new();
        }

        let total_size = env_size + tools_size + if has_tools { 30 } else { 0 };
        let mut output = String::with_capacity(total_size);

        // Environment section
        if let Some(ref dir) = self.working_directory {
            output.push_str("# Environment\n\n");
            output.push_str("Working directory: ");
            output.push_str(dir);
            output.push_str("\n\n");
        }

        // Tool section
        if has_tools {
            output.push_str("# Tool Usage Guidelines\n\n");

            for entry in self.entries {
                output.push_str("## ");
                let mut chars = entry.name.chars();
                if let Some(first) = chars.next() {
                    output.push(first.to_ascii_uppercase());
                    output.push_str(chars.as_str());
                }
                output.push_str(" Tool\n\n");
                output.push_str(entry.context);
                output.push_str("\n\n");
            }
        }

        output.truncate(output.trim_end().len());
        output
    }
}

/// Extension trait for placeholder substitution on preamble strings.
///
/// Provides simple `{key}` placeholder replacement after building a preamble.
/// Unmatched placeholders are left as-is.
///
/// # Example
///
/// ```rust
/// use llm_coding_tools_core::preamble::Substitute;
///
/// let preamble = "Available agents: {agents}".to_string();
/// let result = preamble
///     .substitute("agents", "code-review, research")
///     .substitute("missing", "ignored");
///
/// assert_eq!(result, "Available agents: code-review, research");
/// ```
pub trait Substitute {
    /// Replaces `{key}` placeholder with the given value.
    ///
    /// Returns a new String with the substitution applied.
    /// If the placeholder is not found, returns the string unchanged.
    fn substitute(self, key: &str, value: &str) -> String;

    /// Replaces multiple `{key}` placeholders with their values.
    ///
    /// Accepts an iterator of (key, value) pairs.
    fn substitute_all<'a>(
        self,
        substitutions: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> String;
}

impl Substitute for String {
    #[inline]
    fn substitute(self, key: &str, value: &str) -> String {
        let placeholder = format!("{{{}}}", key);
        self.replace(&placeholder, value)
    }

    fn substitute_all<'a>(
        mut self,
        substitutions: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> String {
        for (key, value) in substitutions {
            let placeholder = format!("{{{}}}", key);
            self = self.replace(&placeholder, value);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTool {
        id: u32,
    }

    impl ToolContext for MockTool {
        const NAME: &'static str = "mock";
        fn context(&self) -> &'static str {
            "Mock tool context."
        }
    }

    #[test]
    fn empty_builder_returns_empty_string() {
        let preamble = PreambleBuilder::<false>::new().build();
        assert!(preamble.is_empty());
    }

    #[test]
    fn track_returns_tool_unchanged() {
        let mut pb = PreambleBuilder::<false>::new();
        let tool = MockTool { id: 42 };
        let returned = pb.track(tool);
        assert_eq!(returned.id, 42);
    }

    #[test]
    fn single_tool_formats_correctly() {
        let mut pb = PreambleBuilder::<false>::new();
        let _ = pb.track(MockTool { id: 1 });
        let preamble = pb.build();

        assert!(preamble.contains("# Tool Usage Guidelines"));
        assert!(preamble.contains("## Mock Tool"));
        assert!(preamble.contains("Mock tool context."));
    }

    #[test]
    fn multiple_tools_preserve_order() {
        struct OtherTool;
        impl ToolContext for OtherTool {
            const NAME: &'static str = "other";
            fn context(&self) -> &'static str {
                "Other context."
            }
        }

        let mut pb = PreambleBuilder::<false>::new();
        let _ = pb.track(MockTool { id: 1 });
        let _ = pb.track(OtherTool);
        let preamble = pb.build();

        let mock_pos = preamble.find("## Mock Tool").unwrap();
        let other_pos = preamble.find("## Other Tool").unwrap();
        assert!(
            mock_pos < other_pos,
            "Tools should appear in insertion order"
        );
    }

    #[test]
    fn builder_without_env_omits_environment_section() {
        let mut pb = PreambleBuilder::<false>::new();
        let _ = pb.track(MockTool { id: 1 });
        let preamble = pb.build();

        assert!(!preamble.contains("# Environment"));
        assert!(!preamble.contains("Working directory"));
        assert!(preamble.contains("# Tool Usage Guidelines"));
    }

    #[test]
    fn builder_with_env_includes_environment_section() {
        let mut pb = PreambleBuilder::<true>::new().working_directory("/home/user/project");
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
    fn builder_with_env_no_working_dir_no_tools_returns_empty() {
        let pb = PreambleBuilder::<true>::new();
        let preamble = pb.build();
        assert!(preamble.is_empty());
    }

    #[test]
    fn builder_with_env_and_working_dir_but_no_tools() {
        // Environment section should render even without tools tracked
        let pb = PreambleBuilder::<true>::new().working_directory("/home/user/project");
        let preamble = pb.build();

        assert!(preamble.contains("# Environment"));
        assert!(preamble.contains("Working directory: /home/user/project"));
        assert!(!preamble.contains("# Tool Usage Guidelines"));
    }

    #[test]
    fn working_directory_accepts_runtime_string() {
        // Simulates std::env::current_dir().unwrap().display().to_string()
        let runtime_path = String::from("/runtime/computed/path");
        let pb = PreambleBuilder::<true>::new().working_directory(runtime_path);
        let preamble = pb.build();

        assert!(preamble.contains("Working directory: /runtime/computed/path"));
    }

    #[test]
    fn working_directory_accepts_str() {
        let pb = PreambleBuilder::<true>::new().working_directory("/static/path");
        let preamble = pb.build();

        assert!(preamble.contains("Working directory: /static/path"));
    }

    #[test]
    fn substitute_replaces_single_placeholder() {
        use super::Substitute;

        let text = "Hello {name}!".to_string();
        let result = text.substitute("name", "World");
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn substitute_leaves_unmatched_placeholders() {
        use super::Substitute;

        let text = "Hello {name}, welcome to {place}!".to_string();
        let result = text.substitute("name", "Alice");
        assert_eq!(result, "Hello Alice, welcome to {place}!");
    }

    #[test]
    fn substitute_handles_empty_value() {
        use super::Substitute;

        let text = "Prefix{middle}Suffix".to_string();
        let result = text.substitute("middle", "");
        assert_eq!(result, "PrefixSuffix");
    }

    #[test]
    fn substitute_all_replaces_multiple() {
        use super::Substitute;

        let text = "Hello {name}, welcome to {place}!".to_string();
        let result = text.substitute_all([("name", "Alice"), ("place", "Wonderland")]);
        assert_eq!(result, "Hello Alice, welcome to Wonderland!");
    }

    #[test]
    fn substitute_no_placeholder_returns_unchanged() {
        use super::Substitute;

        let text = "No placeholders here".to_string();
        let result = text.substitute("missing", "value");
        assert_eq!(result, "No placeholders here");
    }

    #[test]
    fn generic_flag_is_compile_time() {
        // This test verifies the generic works at compile time
        // If it compiles, the generic system works
        let _pb_no_env: PreambleBuilder<false> = PreambleBuilder::new();
        let _pb_with_env: PreambleBuilder<true> = PreambleBuilder::new();

        // Type inference defaults to false
        let _pb_default: PreambleBuilder = PreambleBuilder::new();
    }

    #[test]
    fn backwards_compatibility_existing_api() {
        // Existing code should work unchanged
        let mut pb = PreambleBuilder::<false>::new();
        let _ = pb.track(MockTool { id: 1 });
        let preamble = pb.build();

        assert!(preamble.contains("# Tool Usage Guidelines"));
        assert!(preamble.contains("## Mock Tool"));
    }
}
