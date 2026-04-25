use reloaded_code_core::context;
use reloaded_code_core::{AllowedPathResolver, SystemPromptBuilder};

use super::{definitions, mock_tools, report, PromptArtifacts, PromptCase};

/// Renders one example case and its matching tool-definition payload.
pub fn build_case(case: PromptCase) -> PromptArtifacts {
    let system_prompt = build_system_prompt(case);
    let tool_definitions = definitions::tool_definitions_for_case(case);
    let tool_definition_payload = serde_json::to_string(&tool_definitions).unwrap();
    let guideline_sections = report::collect_guideline_sections(&system_prompt);

    PromptArtifacts {
        system_prompt,
        tool_definitions,
        tool_definition_payload,
        guideline_sections,
    }
}

fn build_system_prompt(case: PromptCase) -> String {
    let mut builder = base_builder(case);
    mock_tools::track_case_tools(&mut builder, case);
    builder.build()
}

fn base_builder(case: PromptCase) -> SystemPromptBuilder {
    let mut builder = SystemPromptBuilder::new().system_prompt(case.system_prompt);
    if let Some(working_directory) = case.working_directory {
        builder = builder.working_directory(working_directory);
    }
    if !case.allowed_paths.is_empty() {
        let resolver = AllowedPathResolver::from_canonical(case.allowed_paths.iter().copied());
        builder = builder.allowed_paths(&resolver);
    }
    if case.include_git_workflow {
        builder = builder.add_context("Git Workflow", context::GIT_WORKFLOW);
    }
    if case.include_github_cli {
        builder = builder.add_context("GitHub CLI", context::GITHUB_CLI);
    }
    builder
}
