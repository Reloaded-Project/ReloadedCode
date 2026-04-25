//! System prompt preview - demonstrates a smaller readonly prompt.
//!
//! Run: cargo run --example system_prompt_preview_readonly -p reloaded-code-core

mod system_prompt;

use reloaded_code_core::context::PathMode;
use system_prompt::{
    build_case, print_footprint, print_ranked_sizes, print_tool_definitions, section_sizes,
    GrepConfig, PromptCase, ReadConfig,
};

fn main() {
    let readonly = build_case(readonly_case());

    println!("{}", readonly.system_prompt);

    println!("\n{}", "=".repeat(60));
    print_footprint("Static request footprint", &readonly);
    print_ranked_sizes("Largest guideline sections:", &section_sizes(&readonly));
    print_ranked_sizes("Largest tool definitions:", &readonly.definition_sizes());
    print_tool_definitions(&readonly);
}

const SYSTEM_PROMPT: &str = "# System Instructions\n\nYou are a helpful coding assistant. Gather relevant information and report concise findings.";

fn readonly_case() -> PromptCase {
    PromptCase {
        system_prompt: SYSTEM_PROMPT,
        working_directory: Some("/home/user/project"),
        allowed_paths: &[],
        include_git_workflow: false,
        include_github_cli: false,
        read: Some(ReadConfig {
            path_mode: PathMode::Absolute,
            line_numbers: false,
        }),
        write: None,
        edit: None,
        bash: false,
        glob: Some(PathMode::Absolute),
        grep: Some(GrepConfig {
            path_mode: PathMode::Absolute,
            line_numbers: false,
        }),
        webfetch: false,
        todo_write: false,
        todo_read: false,
        task_targets: &[],
    }
}
