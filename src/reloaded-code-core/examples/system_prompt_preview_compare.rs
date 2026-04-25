//! Compares full and readonly system prompt footprints.
//!
//! Run: cargo run --example system_prompt_preview_compare -p reloaded-code-core

mod system_prompt;

use reloaded_code_core::context::PathMode;
use system_prompt::{
    build_case, estimate_tokens, print_footprint, GrepConfig, PromptArtifacts, PromptCase,
    ReadConfig, TaskTarget,
};

fn main() {
    let full = build_case(full_case());
    let no_supplemental = build_case(full_case().without_supplemental());
    let readonly = build_case(readonly_case());

    print_footprint("Full", &full);
    println!();
    print_footprint("Full without supplemental workflow", &no_supplemental);
    println!();
    print_footprint("Readonly", &readonly);

    println!("\nSavings vs full:");
    print_delta("  No supplemental workflow", &full, &no_supplemental);
    print_delta("  Readonly", &full, &readonly);
}

const FULL_SYSTEM_PROMPT: &str = "# System Instructions\n\nYou are a helpful coding assistant. Follow best practices and write clean, maintainable code.";

const READONLY_SYSTEM_PROMPT: &str = "# System Instructions\n\nYou are a helpful coding assistant. Gather relevant information and report concise findings.";

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
        system_prompt: FULL_SYSTEM_PROMPT,
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

fn readonly_case() -> PromptCase {
    PromptCase {
        system_prompt: READONLY_SYSTEM_PROMPT,
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

fn print_delta(label: &str, full: &PromptArtifacts, other: &PromptArtifacts) {
    let prompt_saved = full
        .system_prompt
        .len()
        .saturating_sub(other.system_prompt.len());
    let definitions_saved = full
        .tool_definition_payload
        .len()
        .saturating_sub(other.tool_definition_payload.len());
    let total_saved = full.total_chars().saturating_sub(other.total_chars());

    println!(
        "{label}: -{} prompt chars, -{} definition chars, -{} total chars (~{} tokens)",
        prompt_saved,
        definitions_saved,
        total_saved,
        estimate_tokens(total_saved)
    );
}
