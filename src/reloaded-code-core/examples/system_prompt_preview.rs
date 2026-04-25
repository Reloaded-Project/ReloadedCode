//! System prompt preview - demonstrates a full system prompt and its static cost.
//!
//! Run: cargo run --example system_prompt_preview -p reloaded-code-core

mod system_prompt;

use reloaded_code_core::context::PathMode;
use system_prompt::{
    build_case, print_footprint, print_ranked_sizes, print_tool_definitions, section_sizes,
    GrepConfig, PromptCase, ReadConfig, TaskTarget,
};

fn main() {
    let full = build_case(full_case());
    let without_supplemental = build_case(full_case().without_supplemental());

    println!("{}", full.system_prompt);

    println!("\n{}", "=".repeat(60));
    print_footprint("Static request footprint", &full);
    println!(
        "  Note: provider wrappers and user messages add extra overhead beyond these static counts."
    );
    print_ranked_sizes("Largest guideline sections:", &section_sizes(&full));
    print_ranked_sizes("Largest tool definitions:", &full.definition_sizes());
    print_tool_definitions(&full);

    println!("\nWithout supplemental workflow:");
    print_footprint("  Static request footprint", &without_supplemental);
}

const SYSTEM_PROMPT: &str = "# System Instructions\n\nYou are a helpful coding assistant. Follow best practices and write clean, maintainable code.";

const TASK_TARGETS: &[TaskTarget] = &[
    TaskTarget {
        name: "research",
        description: "Investigate implementation details and report back.",
    },
    TaskTarget {
        name: "review",
        description: "Review code and suggest focused fixes.",
    },
];

fn full_case() -> PromptCase {
    PromptCase {
        system_prompt: SYSTEM_PROMPT,
        working_directory: Some("/home/user/project"),
        allowed_paths: &["/home/user/project", "/tmp"],
        include_git_workflow: true,
        include_github_cli: true,
        read: Some(ReadConfig {
            path_mode: PathMode::Allowed,
            line_numbers: true,
        }),
        write: Some(PathMode::Allowed),
        edit: Some(PathMode::Allowed),
        bash: true,
        glob: Some(PathMode::Allowed),
        grep: Some(GrepConfig {
            path_mode: PathMode::Allowed,
            line_numbers: true,
        }),
        webfetch: true,
        todo_write: true,
        todo_read: true,
        task_targets: TASK_TARGETS,
    }
}
