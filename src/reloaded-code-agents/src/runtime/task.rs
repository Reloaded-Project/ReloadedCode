//! Task delegation helpers backed by [`AgentCatalog`].
//!
//! # Public API
//! - [`summarize_callable_targets`] - Builds summary rows with stable names and descriptions.
//! - [`callable_targets`] - Returns the agents an active agent may delegate to via Task.
//! - [`TaskTargetSummary`] - Stable Task UI metadata for a callable target.

use super::tool_catalog::{ToolCatalogEntry, ToolCatalogKind};
use crate::{AgentCatalog, AgentConfig, AgentMode, RulesetExt};
use ahash::AHashMap;
use reloaded_code_core::permissions::{ExpandError, Ruleset};
use reloaded_code_core::tool_metadata::task as task_meta;
use std::sync::Arc;

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
///
/// # Errors
/// - Returns [`ExpandError`] when any entry in the caller's `permission` map contains invalid patterns
///   (the ruleset is built from the full map via [`Ruleset::from_permission_config`]).
pub fn summarize_callable_targets(
    catalog: &AgentCatalog,
    caller_name: &str,
) -> Result<Vec<TaskTargetSummary>, ExpandError> {
    Ok(summarize_targets(callable_targets(catalog, caller_name)?))
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
///
/// # Errors
/// - Returns [`ExpandError`] when any entry in the caller's `permission` map contains invalid patterns
///   (the ruleset is built from the full map via [`Ruleset::from_permission_config`]).
pub fn callable_targets<'a>(
    catalog: &'a AgentCatalog,
    caller_name: &str,
) -> Result<Vec<&'a AgentConfig>, ExpandError> {
    let Some(caller) = catalog.by_name(caller_name) else {
        return Ok(Vec::new());
    };
    let task_rules = Ruleset::from_permission_config(&caller.permission)?;
    // Sort to give deterministic ordering regardless of catalog iteration order;
    // the result feeds into LLM prompts and cached summaries that must be stable.
    let agents = sorted_agents(catalog);
    Ok(filter_callable_targets(&agents, caller, &task_rules))
}

/// Pre-compute per-agent task caches for the entire catalog in one pass.
///
/// For every agent in `catalog`, resolves which targets it may delegate to via Task
/// and which tools it is allowed to invoke, then returns both mappings keyed by
/// agent name.
///
/// # Arguments
/// - `catalog` - All registered agents.
/// - `permission_rulesets` - Pre-built [`Ruleset`] per agent name. Every agent
///   present in `catalog` **must** have an entry or the function panics.
/// - `tools` - The full tool catalog to filter per caller.
///
/// # Returns
/// A tuple of:
/// - Allowed tools per caller agent name ([`Vec<ToolCatalogEntry>`]).
/// - Callable target summaries per caller agent name ([`Vec<TaskTargetSummary>`]).
///
/// # Panics
/// Panics if any agent in `catalog` lacks a corresponding entry in
/// `permission_rulesets`.
pub(super) fn build_runtime_task_caches(
    catalog: &AgentCatalog,
    permission_rulesets: &AHashMap<String, Arc<Ruleset>>,
    tools: &[ToolCatalogEntry],
) -> (
    AHashMap<String, Vec<ToolCatalogEntry>>,
    AHashMap<String, Vec<TaskTargetSummary>>,
) {
    let mut allowed_tools_by_caller = AHashMap::with_capacity(permission_rulesets.len());
    let mut callable_target_summaries_by_caller =
        AHashMap::with_capacity(permission_rulesets.len());
    let agents = sorted_agents(catalog);

    for caller in catalog.iter() {
        let task_rules = permission_rulesets
            .get(caller.name.as_ref())
            .map(Arc::as_ref)
            .expect("every runtime agent must have a cached ruleset");
        let callable_targets = filter_callable_targets(&agents, caller, task_rules);
        let task_is_callable = !callable_targets.is_empty();

        callable_target_summaries_by_caller
            .insert(caller.name.to_string(), summarize_targets(callable_targets));
        allowed_tools_by_caller.insert(
            caller.name.to_string(),
            collect_allowed_tools(tools, task_rules, task_is_callable),
        );
    }

    (allowed_tools_by_caller, callable_target_summaries_by_caller)
}

fn summarize_targets(callable: Vec<&AgentConfig>) -> Vec<TaskTargetSummary> {
    let mut summaries = Vec::with_capacity(callable.len());

    for target in callable {
        summaries.push(TaskTargetSummary {
            name: target.name.clone(),
            description: target.description.clone(),
        });
    }

    summaries
}

fn filter_callable_targets<'a>(
    agents: &[&'a AgentConfig],
    caller: &AgentConfig,
    task_rules: &Ruleset,
) -> Vec<&'a AgentConfig> {
    let has_explicit_task_permission = caller.permission.contains_key(task_meta::NAME);
    agents
        .iter()
        .copied()
        .filter(|t| target_is_callable(t, task_rules, has_explicit_task_permission))
        .collect()
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
    use reloaded_code_core::permissions::{ExpandError, PermissionAction};
    use reloaded_code_core::tool_metadata::{
        bash as bash_meta, read as read_meta, task as task_meta, write as write_meta,
    };

    type TestResult = Result<(), ExpandError>;

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
    fn callable_targets_returns_empty_for_unknown_caller() -> TestResult {
        let catalog = AgentCatalog::from_entries([agent(
            "agent-a",
            AgentMode::All,
            "Agent A",
            allow_tools(&[task_meta::NAME]),
        )]);

        let targets = callable_targets(&catalog, "nonexistent")?;
        assert!(targets.is_empty());
        Ok(())
    }

    /// Primary-mode agents are excluded from callable targets.
    #[test]
    fn callable_targets_filters_primary_targets_even_when_allowed() -> TestResult {
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

        let targets = callable_targets(&catalog, "caller")?;
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"all-target"));
        assert!(names.contains(&"subagent-target"));
        assert!(!names.contains(&"primary-target"));
        assert!(names.contains(&"caller"));
        Ok(())
    }

    /// Self-delegation is allowed when mode and permission both permit.
    #[test]
    fn callable_targets_keeps_self_when_mode_and_permission_allow_it() -> TestResult {
        let catalog = AgentCatalog::from_entries([agent(
            "self-agent",
            AgentMode::All,
            "Self Agent",
            allow_tools(&[task_meta::NAME]),
        )]);

        let targets = callable_targets(&catalog, "self-agent")?;
        assert!(targets.iter().any(|t| t.name.as_ref() == "self-agent"));
        Ok(())
    }

    /// Without explicit `permission.task`, Task defaults to all non-primary targets.
    #[test]
    fn callable_targets_default_to_all_non_primary_when_task_permission_is_absent() -> TestResult {
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

        let targets = callable_targets(&catalog, "caller")?;
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(names, vec!["all-target", "subagent-target"]);
        Ok(())
    }

    /// Wildcard patterns are evaluated; specific patterns override wildcards.
    #[test]
    fn callable_targets_honor_wildcard_and_specific_rules_in_order() -> TestResult {
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

        let targets = callable_targets(&catalog, "caller")?;
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"review-agent"));
        assert!(!names.contains(&"other-agent"));
        Ok(())
    }

    /// Later patterns take precedence (last-match-wins).
    #[test]
    fn callable_targets_use_last_match_wins_precedence() -> TestResult {
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

        let targets = callable_targets(&catalog, "caller")?;
        let names: Vec<_> = targets.iter().map(|t| t.name.as_ref()).collect();
        assert!(!names.contains(&"review-agent"));
        assert!(!names.contains(&"other-agent"));
        Ok(())
    }

    /// OpenCode-style task allowlists support both exact names and wildcards.
    #[test]
    fn callable_targets_support_opencode_style_allowlists() -> TestResult {
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

        let targets = callable_targets(&catalog, "orchestrator")?;
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
        Ok(())
    }

    /// Summaries are alphabetically sorted and preserve target descriptions.
    #[test]
    fn summaries_are_sorted_and_preserve_target_descriptions() -> TestResult {
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

        let summaries = summarize_callable_targets(&catalog, "caller")?;

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
        Ok(())
    }

    /// Explicit deny on `permission.task` suppresses all task targets.
    #[test]
    fn summaries_return_empty_when_task_is_explicitly_denied() -> TestResult {
        let catalog = AgentCatalog::from_entries([
            agent("caller", AgentMode::All, "Caller", deny_task()),
            agent("target", AgentMode::All, "Target", IndexMap::new()),
        ]);

        let summaries = summarize_callable_targets(&catalog, "caller")?;
        assert!(summaries.is_empty());
        Ok(())
    }

    #[test]
    fn allowed_tools_keeps_task_when_a_target_is_callable() -> TestResult {
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
            .build()?;

        let names: Vec<_> = runtime
            .allowed_tools("caller")
            .iter()
            .map(|t| t.name)
            .collect();

        assert_eq!(names, vec![task_meta::NAME]);
        Ok(())
    }

    #[test]
    fn allowed_tools_omits_task_when_no_targets_are_callable() -> TestResult {
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
            .build()?;

        let names: Vec<_> = runtime
            .allowed_tools("caller")
            .iter()
            .map(|t| t.name)
            .collect();

        assert!(names.contains(&read_meta::NAME));
        assert!(!names.contains(&task_meta::NAME));
        Ok(())
    }
}
