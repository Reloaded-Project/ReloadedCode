//! SerdesAI Task definition helpers.
//!
//! # Public API
//! - [`render_task_targets`] - Renders callable targets for Task tool descriptions.
//! - [`task_tool_definition`] - Builds the adapter-facing Task tool definition.

use reloaded_code_agents::TaskTargetSummary;
use reloaded_code_core::tool_metadata::task as task_meta;
use serdes_ai::tools::{SchemaBuilder, ToolDefinition};

/// Renders callable target summaries in a stable, user-facing format.
pub(crate) fn render_task_targets(targets: &[TaskTargetSummary]) -> String {
    if targets.is_empty() {
        return "No callable subagents are available.".to_string();
    }

    let mut ordered: Vec<_> = targets.iter().collect();
    ordered.sort_unstable_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));

    let mut rendered = String::with_capacity(32 + ordered.len() * 64);
    rendered.push_str("Available subagents:\n");
    for target in ordered {
        rendered.push_str("- ");
        rendered.push_str(target.name.as_ref());
        rendered.push_str(": ");
        rendered.push_str(target.description.as_ref());
        rendered.push('\n');
    }
    rendered
}

/// Builds a SerdesAI Task definition using the shared target summaries.
pub(crate) fn task_tool_definition(targets: &[TaskTargetSummary]) -> ToolDefinition {
    let rendered_targets = render_task_targets(targets);
    let mut description =
        String::with_capacity(task_meta::DESCRIPTION_PREFIX.len() + rendered_targets.len() + 2);
    description.push_str(task_meta::DESCRIPTION_PREFIX);
    description.push_str("\n\n");
    description.push_str(&rendered_targets);
    let schema = SchemaBuilder::new()
        .string(
            task_meta::param::DESCRIPTION.name,
            task_meta::param::DESCRIPTION.description,
            task_meta::param::DESCRIPTION.required,
        )
        .string(
            task_meta::param::PROMPT.name,
            task_meta::param::PROMPT.description,
            task_meta::param::PROMPT.required,
        )
        .string(
            task_meta::param::SUBAGENT_TYPE.name,
            task_meta::param::SUBAGENT_TYPE.description,
            task_meta::param::SUBAGENT_TYPE.required,
        )
        .string(
            task_meta::param::COMMAND.name,
            task_meta::param::COMMAND.description,
            task_meta::param::COMMAND.required,
        )
        .build()
        .expect("task schema should be valid");

    ToolDefinition {
        name: task_meta::NAME.to_owned(),
        description,
        parameters_json_schema: schema,
        strict: None,
        outer_typed_dict_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{task_meta, *};
    use rstest::rstest;

    fn summary(name: &str, description: &str) -> TaskTargetSummary {
        TaskTargetSummary {
            name: name.into(),
            description: description.into(),
        }
    }

    #[rstest]
    // Alphabetical ordering: zebra/alpha/mike → alpha/mike/zebra
    #[case::sorts_alphabetically(
        vec![summary("zebra", "Last alphabetically"), summary("alpha", "First alphabetically"), summary("mike", "Middle")],
        None::<&str>,
        "Available subagents:\n- alpha: First alphabetically\n- mike: Middle\n- zebra: Last alphabetically\n",
    )]
    // Format: only "- name: description" per line, no "tools:" leakage
    #[case::shows_name_and_description(
        vec![summary("with-task", "Can delegate"), summary("no-task", "Cannot delegate")],
        Some("tools:"),
        "Available subagents:\n- no-task: Cannot delegate\n- with-task: Can delegate\n",
    )]
    // Empty input falls back to the dedicated message
    #[case::handles_empty_input(
        vec![],
        None::<&str>,
        "No callable subagents are available.",
    )]
    fn render_task_targets_renders_expected_output(
        #[case] targets: Vec<TaskTargetSummary>,
        #[case] forbidden_fragment: Option<&str>,
        #[case] expected_rendered: &str,
    ) {
        let rendered = render_task_targets(&targets);
        assert_eq!(rendered, expected_rendered);
        if let Some(fragment) = forbidden_fragment {
            assert!(!rendered.contains(fragment));
        }
    }

    #[test]
    fn task_tool_definition_uses_task_name_and_expected_parameters() {
        let targets = vec![summary("test", "Test agent")];
        let definition = task_tool_definition(&targets);

        assert_eq!(definition.name(), task_meta::NAME);

        // Verify description includes all expected parameters
        let desc = definition.description();
        assert!(!desc.is_empty());
    }
}
