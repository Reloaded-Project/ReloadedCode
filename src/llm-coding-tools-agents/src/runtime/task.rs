//! Task delegation helpers backed by [`AgentCatalog`].
//!
//! # Public API
//! - [`summarize_callable_targets`] - Builds summary rows with stable names and descriptions.
//! - [`callable_targets`] - Returns the agents an active agent may delegate to via Task.
//! - [`TaskTargetSummary`] - Stable Task UI metadata for a callable target.

use super::state::AgentRuntime;
use super::tool_catalog::{ToolCatalogEntry, ToolCatalogKind};
use crate::{AgentCatalog, AgentConfig, AgentMode, RulesetExt};
use llm_coding_tools_core::permissions::Ruleset;
use llm_coding_tools_core::tool_metadata::task as task_meta;

/// Compact metadata used to describe one callable Task target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTargetSummary {
    /// Stable target name.
    pub name: Box<str>,
    /// Human-readable target description.
    pub description: Box<str>,
}

/// For each agent `caller_name` can delegate to, returns its name and description.
/// Results are in consistent alphabetical order.
///
/// # Params
/// - `catalog` - All registered agents.
/// - `caller_name` - Name of the agent that wants to delegate.
///
/// # Returns
/// One [`TaskTargetSummary`] per callable target, sorted by name. Empty if
/// `caller_name` is not in the catalog or no non-primary targets are available.
pub fn summarize_callable_targets(
    catalog: &AgentCatalog,
    caller_name: &str,
) -> Vec<TaskTargetSummary> {
    let callable = callable_targets(catalog, caller_name);
    let mut summaries = Vec::with_capacity(callable.len());

    // Copy stable Task metadata into owned summaries.
    for target in callable {
        summaries.push(TaskTargetSummary {
            name: target.name.clone(),
            description: target.description.clone(),
        });
    }

    summaries
}

/// Returns the agents that `caller_name` (the currently running agent) may delegate to via Task.
///
/// # Params
/// - `catalog` - All registered agents.
/// - `caller_name` - Name of the agent that wants to delegate.
///
/// # Returns
/// Agents the caller may delegate to, sorted alphabetically. Empty if `caller_name`
/// is not in the catalog or no non-primary targets are available.
///
/// When the caller does not define `permission.task`, Task defaults to all
/// `mode: all` and `mode: subagent` targets for OpenCode compatibility. When
/// `permission.task` is present, its rules filter target names with the normal
/// last-match-wins permission semantics.
pub fn callable_targets<'a>(catalog: &'a AgentCatalog, caller_name: &str) -> Vec<&'a AgentConfig> {
    let Some(caller) = catalog.by_name(caller_name) else {
        return Vec::new();
    };

    let agents = sorted_agents(catalog);
    let task_rules = Ruleset::from_permission_config(&caller.permission);
    let has_explicit_task_permission = caller.permission.contains_key(task_meta::NAME);
    let mut targets = Vec::with_capacity(agents.len());

    // Keep only non-primary targets that survive `permission.task` filtering.
    for target in agents {
        if target_is_callable(target, &task_rules, has_explicit_task_permission) {
            targets.push(target);
        }
    }

    targets
}

fn sorted_agents(catalog: &AgentCatalog) -> Vec<&AgentConfig> {
    let mut agents: Vec<_> = catalog.iter().collect();
    agents.sort_unstable_by(|left, right| left.name.as_ref().cmp(right.name.as_ref()));
    agents
}

fn target_is_callable(
    target: &AgentConfig,
    task_rules: &Ruleset,
    has_explicit_task_permission: bool,
) -> bool {
    matches!(target.mode, AgentMode::All | AgentMode::Subagent)
        && (!has_explicit_task_permission
            || task_rules.is_allowed(task_meta::NAME, target.name.as_ref()))
}

pub(super) fn resolve_allowed_tools(
    runtime: &AgentRuntime,
    caller_name: &str,
) -> Vec<ToolCatalogEntry> {
    let Some(caller) = runtime.catalog().by_name(caller_name) else {
        return Vec::new();
    };

    let agents = sorted_agents(runtime.catalog());
    let task_rules = Ruleset::from_permission_config(&caller.permission);
    let has_explicit_task_permission = caller.permission.contains_key(task_meta::NAME);
    let mut task_is_callable = false;

    // Expose `task` only when at least one delegated target remains callable.
    for target in agents {
        if target_is_callable(target, &task_rules, has_explicit_task_permission) {
            task_is_callable = true;
            break;
        }
    }

    collect_allowed_tools(runtime.tools(), &task_rules, task_is_callable)
}

fn collect_allowed_tools(
    tools: &[ToolCatalogEntry],
    task_rules: &Ruleset,
    task_is_callable: bool,
) -> Vec<ToolCatalogEntry> {
    let mut allowed = Vec::with_capacity(tools.len());

    for entry in tools {
        let is_allowed = match entry.kind {
            // Task is target-scoped, so wildcard tool filtering alone is not enough.
            ToolCatalogKind::Task => task_is_callable,
            _ => task_rules.is_allowed(entry.name, "*"),
        };

        if is_allowed {
            allowed.push(*entry);
        }
    }

    allowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PermissionRule;
    use crate::{AgentConfig, AgentMode, AgentRuntimeBuilder, AgentToolSettings};
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use llm_coding_tools_core::permissions::PermissionAction;
    use llm_coding_tools_core::tool_metadata::{
        bash as bash_meta, read as read_meta, task as task_meta, write as write_meta,
    };

    fn agent(
        name: &str,
        mode: AgentMode,
        description: &str,
        permission: IndexMap<String, PermissionRule>,
    ) -> AgentConfig {
        AgentConfig {
            name: name.into(),
            mode,
            description: description.into(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission,
            options: AHashMap::new(),
            tool_settings: AgentToolSettings::default(),
            prompt: Default::default(),
        }
    }

    fn allow_tools(names: &[&str]) -> IndexMap<String, PermissionRule> {
        names
            .iter()
            .map(|n| ((*n).into(), PermissionRule::Action(PermissionAction::Allow)))
            .collect()
    }

    fn pattern_task(patterns: &[(&str, PermissionAction)]) -> IndexMap<String, PermissionRule> {
        let mut map = IndexMap::new();
        for (pattern, action) in patterns {
            map.insert(pattern.to_string(), *action);
        }
        IndexMap::from([(task_meta::NAME.into(), PermissionRule::Pattern(map))])
    }

    fn deny_task() -> IndexMap<String, PermissionRule> {
        IndexMap::from([(
            task_meta::NAME.into(),
            PermissionRule::Action(PermissionAction::Deny),
        )])
    }

    /// Unknown callers yield no targets.
    #[test]
    fn callable_targets_returns_empty_for_unknown_caller() {
        let catalog = AgentCatalog::from_entries([agent(
            "agent-a",
            AgentMode::All,
            "Agent A",
            allow_tools(&[task_meta::NAME]),
        )]);

        let targets = callable_targets(&catalog, "nonexistent");
        assert!(targets.is_empty());
    }

    /// Primary-mode agents are excluded from callable targets.
    #[test]
    fn callable_targets_filters_primary_targets_even_when_allowed() {
        let catalog = AgentCatalog::from_entries([
            agent(
                "caller",
                AgentMode::All,
                "Caller",
                allow_tools(&[task_meta::NAME]),
            ),
            agent("all-target", AgentMode::All, "All Target", IndexMap::new()),
            agent(
                "subagent-target",
                AgentMode::Subagent,
                "Subagent Target",
                IndexMap::new(),
            ),
            agent(
                "primary-target",
                AgentMode::Primary,
                "Primary Target",
                IndexMap::new(),
            ),
        ]);

        let targets = callable_targets(&catalog, "caller");
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"all-target"));
        assert!(names.contains(&"subagent-target"));
        assert!(!names.contains(&"primary-target"));
        assert!(names.contains(&"caller"));
    }

    /// Self-delegation is allowed when mode and permission both permit.
    #[test]
    fn callable_targets_keeps_self_when_mode_and_permission_allow_it() {
        let catalog = AgentCatalog::from_entries([agent(
            "self-agent",
            AgentMode::All,
            "Self Agent",
            allow_tools(&[task_meta::NAME]),
        )]);

        let targets = callable_targets(&catalog, "self-agent");
        assert!(targets.iter().any(|t| t.name.as_ref() == "self-agent"));
    }

    /// Without explicit `permission.task`, Task defaults to all non-primary targets.
    #[test]
    fn callable_targets_default_to_all_non_primary_when_task_permission_is_absent() {
        let catalog = AgentCatalog::from_entries([
            agent(
                "caller",
                AgentMode::Primary,
                "Caller",
                allow_tools(&[read_meta::NAME]),
            ),
            agent("all-target", AgentMode::All, "All Target", IndexMap::new()),
            agent(
                "subagent-target",
                AgentMode::Subagent,
                "Subagent Target",
                IndexMap::new(),
            ),
            agent(
                "primary-target",
                AgentMode::Primary,
                "Primary Target",
                IndexMap::new(),
            ),
        ]);

        let targets = callable_targets(&catalog, "caller");
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, vec!["all-target", "subagent-target"]);
    }

    /// Wildcard patterns are evaluated; specific patterns override wildcards.
    #[test]
    fn callable_targets_honor_wildcard_and_specific_rules_in_order() {
        let catalog = AgentCatalog::from_entries([
            agent(
                "caller",
                AgentMode::All,
                "Caller",
                pattern_task(&[
                    ("*", PermissionAction::Deny),
                    ("review-*", PermissionAction::Allow),
                ]),
            ),
            agent(
                "review-agent",
                AgentMode::All,
                "Review Agent",
                IndexMap::new(),
            ),
            agent(
                "other-agent",
                AgentMode::All,
                "Other Agent",
                IndexMap::new(),
            ),
        ]);

        let targets = callable_targets(&catalog, "caller");
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"review-agent"));
        assert!(!names.contains(&"other-agent"));
    }

    /// Later patterns take precedence (last-match-wins).
    #[test]
    fn callable_targets_use_last_match_wins_precedence() {
        let catalog = AgentCatalog::from_entries([
            agent(
                "caller",
                AgentMode::All,
                "Caller",
                pattern_task(&[
                    ("review-*", PermissionAction::Allow),
                    ("*", PermissionAction::Deny),
                ]),
            ),
            agent(
                "review-agent",
                AgentMode::All,
                "Review Agent",
                IndexMap::new(),
            ),
            agent(
                "other-agent",
                AgentMode::All,
                "Other Agent",
                IndexMap::new(),
            ),
        ]);

        let targets = callable_targets(&catalog, "caller");
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert!(!names.contains(&"review-agent"));
        assert!(!names.contains(&"other-agent"));
    }

    /// OpenCode-style task allowlists support both exact names and wildcards.
    #[test]
    fn callable_targets_support_opencode_style_allowlists() {
        let catalog = AgentCatalog::from_entries([
            agent(
                "orchestrator",
                AgentMode::Primary,
                "Orchestrator",
                pattern_task(&[
                    ("*", PermissionAction::Deny),
                    ("commit", PermissionAction::Allow),
                    ("coderabbit", PermissionAction::Allow),
                    ("orchestrator-*", PermissionAction::Allow),
                ]),
            ),
            agent("commit", AgentMode::Subagent, "Commit", IndexMap::new()),
            agent(
                "coderabbit",
                AgentMode::Subagent,
                "CodeRabbit",
                IndexMap::new(),
            ),
            agent(
                "orchestrator-runner",
                AgentMode::Subagent,
                "Runner",
                IndexMap::new(),
            ),
            agent(
                "orchestrator-builder",
                AgentMode::Subagent,
                "Builder",
                IndexMap::new(),
            ),
            agent("general", AgentMode::Subagent, "General", IndexMap::new()),
            agent(
                "primary-only",
                AgentMode::Primary,
                "Primary Only",
                IndexMap::new(),
            ),
        ]);

        let targets = callable_targets(&catalog, "orchestrator");
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(
            names,
            vec![
                "coderabbit",
                "commit",
                "orchestrator-builder",
                "orchestrator-runner",
            ]
        );
    }

    /// Summaries are alphabetically sorted and preserve target descriptions.
    #[test]
    fn summaries_are_sorted_and_preserve_target_descriptions() {
        let catalog = AgentCatalog::from_entries([
            agent(
                "zebra",
                AgentMode::All,
                "Zebra description",
                allow_tools(&[read_meta::NAME, bash_meta::NAME]),
            ),
            agent(
                "alpha",
                AgentMode::All,
                "Alpha description",
                allow_tools(&[write_meta::NAME]),
            ),
            agent(
                "caller",
                AgentMode::All,
                "Caller",
                allow_tools(&[task_meta::NAME]),
            ),
        ]);

        let summaries = summarize_callable_targets(&catalog, "caller");

        let names: Vec<&str> = summaries.iter().map(|s| s.name.as_ref()).collect();
        assert_eq!(names, vec!["alpha", "caller", "zebra"]);

        let alpha_summary = summaries
            .iter()
            .find(|s| s.name.as_ref() == "alpha")
            .unwrap();
        assert_eq!(alpha_summary.description.as_ref(), "Alpha description");

        let zebra_summary = summaries
            .iter()
            .find(|s| s.name.as_ref() == "zebra")
            .unwrap();
        assert_eq!(zebra_summary.description.as_ref(), "Zebra description");
    }

    /// Explicit deny on `permission.task` suppresses all task targets.
    #[test]
    fn summaries_return_empty_when_task_is_explicitly_denied() {
        let catalog = AgentCatalog::from_entries([
            agent("caller", AgentMode::All, "Caller", deny_task()),
            agent("target", AgentMode::All, "Target", IndexMap::new()),
        ]);

        let summaries = summarize_callable_targets(&catalog, "caller");
        assert!(summaries.is_empty());
    }

    #[test]
    fn allowed_tools_keeps_task_when_a_target_is_callable() {
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::Primary,
                    "Caller",
                    pattern_task(&[
                        ("*", PermissionAction::Deny),
                        ("reader", PermissionAction::Allow),
                    ]),
                ),
                agent("reader", AgentMode::Subagent, "Reader", IndexMap::new()),
                agent("writer", AgentMode::Subagent, "Writer", IndexMap::new()),
            ]))
            .build();

        let names: Vec<_> = runtime
            .allowed_tools("caller")
            .iter()
            .map(|t| t.name)
            .collect();

        assert_eq!(names, vec![task_meta::NAME]);
    }

    #[test]
    fn allowed_tools_omits_task_when_no_targets_are_callable() {
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([
                agent(
                    "caller",
                    AgentMode::Primary,
                    "Caller",
                    allow_tools(&[task_meta::NAME, read_meta::NAME]),
                ),
                agent(
                    "primary-target",
                    AgentMode::Primary,
                    "Primary",
                    IndexMap::new(),
                ),
            ]))
            .build();

        let names: Vec<_> = runtime
            .allowed_tools("caller")
            .iter()
            .map(|t| t.name)
            .collect();

        assert!(names.contains(&read_meta::NAME));
        assert!(!names.contains(&task_meta::NAME));
    }
}
