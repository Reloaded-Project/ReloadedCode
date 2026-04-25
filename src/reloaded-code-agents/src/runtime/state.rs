//! Holds your loaded agents, default settings, Task settings, and available tools.
//!
//! ## Public API
//!
//! - [`AgentRuntime`] — Container for loaded agents, defaults, Task settings, and tools.
//! - [`AgentDefaults`] — Fallback settings when an agent doesn't specify them.

use super::task::{build_runtime_task_caches, TaskTargetSummary};
use super::tool_catalog::ToolCatalogEntry;
use crate::{AgentCatalog, RulesetExt};
use ahash::AHashMap;
use reloaded_code_core::permissions::{ExpandError, Ruleset};
use reloaded_code_core::TaskSettings;
use std::sync::Arc;

/// Default settings used when an agent doesn't specify them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentDefaults {
    /// Default model in `provider/model-id` format.
    pub model: Option<Box<str>>,
    /// Default sampling temperature.
    pub temperature: Option<f32>,
    /// Default nucleus sampling top-p.
    pub top_p: Option<f32>,
}

impl AgentDefaults {
    /// Creates defaults with only a model specified; temperature and top_p inherit provider defaults.
    #[inline]
    pub fn with_model(model: impl Into<Box<str>>) -> Self {
        Self {
            model: Some(model.into()),
            temperature: None,
            top_p: None,
        }
    }
}

/// Your loaded agents plus their default settings, Task settings, and available tools.
#[derive(Debug, Clone)]
pub struct AgentRuntime {
    catalog: AgentCatalog,
    defaults: AgentDefaults,
    task_settings: TaskSettings,
    tools: Vec<ToolCatalogEntry>,
    permission_rulesets: AHashMap<String, Arc<Ruleset>>,
    allowed_tools_by_caller: AHashMap<String, Vec<ToolCatalogEntry>>,
    callable_target_summaries_by_caller: AHashMap<String, Vec<TaskTargetSummary>>,
}

impl AgentRuntime {
    #[inline]
    pub(super) fn from_parts(
        catalog: AgentCatalog,
        defaults: AgentDefaults,
        task_settings: TaskSettings,
        tools: Vec<ToolCatalogEntry>,
    ) -> Result<Self, ExpandError> {
        let permission_rulesets = catalog
            .iter()
            .map(|agent| {
                Ok((
                    agent.name.to_string(),
                    Arc::new(Ruleset::from_permission_config(&agent.permission)?),
                ))
            })
            .collect::<Result<AHashMap<_, _>, ExpandError>>()?;
        let (allowed_tools_by_caller, callable_target_summaries_by_caller) =
            build_runtime_task_caches(&catalog, &permission_rulesets, &tools);

        Ok(Self {
            catalog,
            defaults,
            task_settings,
            tools,
            permission_rulesets,
            allowed_tools_by_caller,
            callable_target_summaries_by_caller,
        })
    }

    /// Returns the loaded agent definitions.
    #[inline]
    pub fn catalog(&self) -> &AgentCatalog {
        &self.catalog
    }

    /// Returns the default settings.
    #[inline]
    pub fn defaults(&self) -> &AgentDefaults {
        &self.defaults
    }

    /// Returns the shared Task delegation settings.
    #[inline]
    pub fn task_settings(&self) -> TaskSettings {
        self.task_settings
    }

    /// Returns the tools available to agents.
    #[inline]
    pub fn tools(&self) -> &[ToolCatalogEntry] {
        &self.tools
    }

    /// Returns the cached permission ruleset for the named caller.
    ///
    /// The returned [`Arc`] is cheap to clone and reuses the ruleset built when
    /// the runtime was constructed.
    #[inline]
    pub fn permission_ruleset(&self, caller_name: &str) -> Option<Arc<Ruleset>> {
        self.permission_rulesets.get(caller_name).cloned()
    }

    /// Returns the tool entries exposed to the named caller.
    ///
    /// Most tools use the standard wildcard permission check (`permission -> "*"`).
    /// `task` is only included when at least one `mode: all` or `mode: subagent`
    /// target remains callable after applying `permission.task`.
    #[inline]
    pub fn allowed_tools(&self, caller_name: &str) -> &[ToolCatalogEntry] {
        self.allowed_tools_by_caller
            .get(caller_name)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns stable summaries for every agent the named caller may delegate to via Task.
    ///
    /// Only agents with [`AgentMode::All`] or [`AgentMode::Subagent`] are
    /// considered. When the caller defines `permission.task`, targets are
    /// filtered by those rules; otherwise all non-primary agents are included.
    /// Results are sorted alphabetically by target name.
    ///
    /// # Arguments
    ///
    /// * `caller_name` - Name of the agent that wants to delegate.
    ///
    /// # Returns
    ///
    /// A [`TaskTargetSummary`] per callable target. Empty if `caller_name`
    /// is not in the catalog or no targets survive permission filtering.
    ///
    /// [`AgentMode::All`]: crate::AgentMode::All
    /// [`AgentMode::Subagent`]: crate::AgentMode::Subagent
    #[inline]
    pub fn summarize_callable_targets(&self, caller_name: &str) -> &[TaskTargetSummary] {
        self.callable_target_summaries_by_caller
            .get(caller_name)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns whether the named caller may delegate to the given target.
    ///
    /// Looks up the caller's filtered callable-target list and performs a
    /// binary search for `target_name`.
    ///
    /// # Arguments
    /// - `caller_name`: Name of the agent that would originate the delegation.
    /// - `target_name`: Name of the candidate delegate target.
    ///
    /// # Returns
    /// - `true` if `target_name` appears in the caller's permitted target list.
    /// - `false` if the caller is not in the catalog or the target is absent.
    #[inline]
    pub fn can_delegate_to(&self, caller_name: &str, target_name: &str) -> bool {
        self.callable_target_summaries_by_caller
            .get(caller_name)
            .is_some_and(|summaries| {
                summaries
                    .binary_search_by(|summary| summary.name.as_ref().cmp(target_name))
                    .is_ok()
            })
    }
}
