//! System Prompt preview - demonstrates full system prompt generation.
//!
//! Shows how SystemPromptBuilder combines:
//! - Custom system prompts
//! - Environment section (working directory, allowed paths)
//! - Tool usage guidelines (from tracked tools)
//! - Supplemental context (git workflow, GitHub CLI)
//!
//! Run: cargo run --example system_prompt_preview -p llm-coding-tools-core

use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::{context, AllowedPathResolver, SystemPromptBuilder};

fn main() {
    // Use from_canonical to avoid filesystem requirements for the example.
    // In real usage, AllowedPathResolver::new() canonicalizes and validates paths.
    let resolver = AllowedPathResolver::from_canonical(["/home/user/project", "/tmp"]);

    // Build system prompt with all features demonstrated
    let mut pb = SystemPromptBuilder::new()
        .system_prompt(
            "# System Instructions\n\n\
             You are a helpful coding assistant. Follow best practices and \
             write clean, maintainable code.",
        )
        .working_directory("/home/user/project")
        .allowed_paths(&resolver)
        .add_context("Git Workflow", context::GIT_WORKFLOW)
        .add_context("GitHub CLI", context::GITHUB_CLI);

    // Track tools - in real usage this would be:
    //   .tool(pb.track(ReadTool::new()))
    // For the preview, we just register them without using the returned tool.
    let _ = pb.track(MockReadTool);
    let _ = pb.track(MockWriteTool);
    let _ = pb.track(MockEditTool);
    let _ = pb.track(MockBashTool);
    let _ = pb.track(MockGlobTool);
    let _ = pb.track(MockGrepTool);
    let _ = pb.track(MockWebFetchTool);
    let _ = pb.track(MockTodoWriteTool);
    let _ = pb.track(MockTodoReadTool);

    let preamble = pb.build();

    // Output the preamble
    println!("{preamble}");

    // Show statistics for token estimation
    println!("\n{}", "=".repeat(60));
    println!("Statistics:");
    println!("  Characters: {}", preamble.len());
    println!("  Lines: {}", preamble.lines().count());
    println!("  Estimated tokens: ~{} (chars/4)", preamble.len() / 4);
}

// Mock tools implementing ToolContext for demonstration.
// In real usage, these would be actual tool structs from llm-coding-tools-serdesai.

struct MockReadTool;
impl ToolContext for MockReadTool {
    const NAME: &'static str = "read";
    fn context(&self) -> &'static str {
        context::READ_ALLOWED
    }
}

struct MockWriteTool;
impl ToolContext for MockWriteTool {
    const NAME: &'static str = "write";
    fn context(&self) -> &'static str {
        context::WRITE_ALLOWED
    }
}

struct MockEditTool;
impl ToolContext for MockEditTool {
    const NAME: &'static str = "edit";
    fn context(&self) -> &'static str {
        context::EDIT_ALLOWED
    }
}

struct MockBashTool;
impl ToolContext for MockBashTool {
    const NAME: &'static str = "bash";
    fn context(&self) -> &'static str {
        context::BASH
    }
}

struct MockGlobTool;
impl ToolContext for MockGlobTool {
    const NAME: &'static str = "glob";
    fn context(&self) -> &'static str {
        context::GLOB_ALLOWED
    }
}

struct MockGrepTool;
impl ToolContext for MockGrepTool {
    const NAME: &'static str = "grep";
    fn context(&self) -> &'static str {
        context::GREP_ALLOWED
    }
}

struct MockWebFetchTool;
impl ToolContext for MockWebFetchTool {
    const NAME: &'static str = "webfetch";
    fn context(&self) -> &'static str {
        context::WEBFETCH
    }
}

struct MockTodoWriteTool;
impl ToolContext for MockTodoWriteTool {
    const NAME: &'static str = "todowrite";
    fn context(&self) -> &'static str {
        context::TODO_WRITE
    }
}

struct MockTodoReadTool;
impl ToolContext for MockTodoReadTool {
    const NAME: &'static str = "todoread";
    fn context(&self) -> &'static str {
        context::TODO_READ
    }
}
