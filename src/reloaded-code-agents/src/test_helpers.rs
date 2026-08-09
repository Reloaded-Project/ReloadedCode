use crate::types::PermissionRule;
use crate::{AgentConfig, AgentMode, AgentToolSettings};
use ahash::AHashMap;
use indexmap::IndexMap;
use reloaded_code_core::permissions::PermissionAction;
use reloaded_code_core::tool_metadata::task as task_meta;

/// Build an [`AgentConfig`] for tests.
///
/// # Parameters
/// - `name` - agent name
/// - `mode` - agent operating mode
/// - `description` - agent description
/// - `permission` - initial permission rules
///
/// # Returns
/// A populated [`AgentConfig`] with all other fields set to defaults.
pub(crate) fn agent(
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

/// Map tool names to `Allow` permission rules.
///
/// # Parameters
/// - `names` - tool names that should be allowed.
///
/// # Returns
/// An ordered map of name → `PermissionRule::Action(Allow)`.
pub(crate) fn allow_tools(names: &[&str]) -> IndexMap<String, PermissionRule> {
    names
        .iter()
        .map(|n| ((*n).into(), PermissionRule::Action(PermissionAction::Allow)))
        .collect()
}

/// Return a deny-all rule for task execution.
///
/// # Returns
/// A single-entry map keyed by [`task_meta::NAME`] with `PermissionRule::Action(Deny)`.
pub(crate) fn deny_task() -> IndexMap<String, PermissionRule> {
    IndexMap::from([(
        task_meta::NAME.into(),
        PermissionRule::Action(PermissionAction::Deny),
    )])
}

/// Build task-scoped pattern permissions.
///
/// Patterns are wrapped under the task metadata name so they apply to task execution.
///
/// # Parameters
/// - `patterns` - ordered pattern/action pairs.
///
/// # Returns
/// A single-entry map keyed by [`task_meta::NAME`] containing [`PermissionRule::Pattern`].
pub(crate) fn pattern_task(
    patterns: &[(&str, PermissionAction)],
) -> IndexMap<String, PermissionRule> {
    let mut map = IndexMap::new();
    for (pattern, action) in patterns {
        map.insert(pattern.to_string(), *action);
    }
    IndexMap::from([(task_meta::NAME.into(), PermissionRule::Pattern(map))])
}
