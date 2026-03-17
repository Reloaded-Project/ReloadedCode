//! Runs delegated Task requests inside SerdesAI.
//!
//! [`TaskHandle`] checks that the caller is allowed to reach the target agent,
//! then builds and runs that agent with the caller's prompt.
//! Each call is independent — no session state is kept between runs.

use crate::agent_runtime::{TaskBuildContext, build_task_enabled_agent};
use llm_coding_tools_agents::{AgentMode, RulesetExt};
use llm_coding_tools_core::permissions::Ruleset;
use llm_coding_tools_core::{
    CredentialLookup, CredentialResolver, TaskInput, TaskOutput, tool_names,
};
use serdes_ai::tools::ToolError;
use std::sync::Arc;

/// Shared Task executor used by the concrete SerdesAI tool.
pub(crate) struct TaskHandle<C: CredentialLookup + Send + Sync + ?Sized = CredentialResolver> {
    context: Arc<TaskBuildContext<C>>,
    current_depth: u8,
}

impl<C> Clone for TaskHandle<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            context: Arc::clone(&self.context),
            current_depth: self.current_depth,
        }
    }
}

impl<C> TaskHandle<C>
where
    C: CredentialLookup + Send + Sync + 'static,
{
    /// Creates a new handle over the shared task-enabled build context.
    #[inline]
    pub(crate) fn new(context: Arc<TaskBuildContext<C>>, current_depth: u8) -> Self {
        Self {
            context,
            current_depth,
        }
    }

    /// Validates the delegation request, builds a task-scoped agent, and runs it.
    ///
    /// # Params
    ///
    /// - `caller_name` — name of the initiating agent (must exist in the catalog).
    /// - `input` — task payload including the [`subagent_type`](TaskInput::subagent_type)
    ///   and prompt.
    ///
    /// # Returns
    ///
    /// A [`TaskOutput`] wrapping the sub-agent's text response.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::ValidationFailed`] when:
    /// - `session_id` is present (task sessions are unsupported).
    /// - The caller is already at the configured maximum Task delegation depth.
    /// - The caller or target agent is missing from the catalog.
    /// - The target uses [`AgentMode::Primary`].
    /// - The caller lacks permission to delegate to the target.
    ///
    /// Returns [`ToolError::ExecutionFailed`] when the sub-agent fails to build or
    /// produce a response.
    pub(crate) async fn execute(
        &self,
        caller_name: &str,
        input: TaskInput,
    ) -> Result<TaskOutput, ToolError> {
        if input.session_id.is_some() {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("session_id".to_string()),
                "task sessions are not supported by this runtime; omit `session_id`",
            ));
        }

        let target_name = input.subagent_type.clone();
        let task_settings = self.context.runtime().task_settings();
        if !task_settings.allows_delegation(self.current_depth) {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                None,
                format!(
                    "task delegation depth {} reached runtime max_task_depth {}; cannot delegate to `{}`",
                    self.current_depth,
                    task_settings.max_depth(),
                    target_name,
                ),
            ));
        }

        self.validate_target(caller_name, &target_name)?;
        let agent = build_task_enabled_agent::<C>(
            self.context.clone(),
            target_name.as_str(),
            self.current_depth.saturating_add(1),
        )
        .map_err(|err| {
            ToolError::execution_failed(format!(
                "failed to build delegated agent `{}`: {err}",
                target_name
            ))
        })?;
        let response = agent.run(input.prompt.as_str(), ()).await.map_err(|err| {
            ToolError::execution_failed(format!("delegated agent `{}` failed: {err}", target_name))
        })?;
        Ok(TaskOutput::new(response.into_output()))
    }

    fn validate_target(&self, caller_name: &str, target_name: &str) -> Result<(), ToolError> {
        let catalog = self.context.runtime().catalog();
        let caller = catalog.by_name(caller_name).ok_or_else(|| {
            ToolError::execution_failed(format!(
                "delegating agent `{caller_name}` disappeared from the runtime catalog"
            ))
        })?;
        let target = catalog.by_name(target_name).ok_or_else(|| {
            ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!("unknown delegated agent `{target_name}`"),
            )
        })?;

        if matches!(target.mode, AgentMode::Primary) {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!(
                    "agent `{target_name}` uses `mode: primary` and cannot be called with task"
                ),
            ));
        }

        // `validate_target` only applies `Ruleset` filtering when `caller.permission`
        // explicitly defines `tool_names::TASK`; without that opt-in, non-Primary
        // targets remain callable for compatibility, while `AgentMode::Primary`
        // targets are always denied above.
        let has_explicit_task_permission = caller.permission.contains_key(tool_names::TASK);
        if has_explicit_task_permission
            && !Ruleset::from_permission_config(&caller.permission)
                .is_allowed(tool_names::TASK, target_name)
        {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!("caller `{caller_name}` is not allowed to delegate to `{target_name}`"),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::TaskBuildContext;
    use ahash::AHashMap;
    use indexmap::IndexMap;
    use llm_coding_tools_agents::{
        AgentCatalog, AgentConfig, AgentDefaults, AgentMode, AgentRuntimeBuilder, PermissionRule,
    };
    use llm_coding_tools_core::CredentialResolver;
    use llm_coding_tools_core::models::{
        Modality, ModelCatalog, ModelInfo, ProviderIdx, ProviderInfo, ProviderModelSource,
        ProviderSource, ProviderType,
    };
    use llm_coding_tools_core::permissions::PermissionAction;
    use llm_coding_tools_core::tool_names;

    fn agent(
        name: &str,
        mode: AgentMode,
        permission: IndexMap<String, PermissionRule>,
    ) -> AgentConfig {
        AgentConfig {
            name: name.into(),
            mode,
            description: format!("{name} description").into(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission,
            options: AHashMap::new(),
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
        IndexMap::from([("task".into(), PermissionRule::Pattern(map))])
    }

    fn catalog() -> ModelCatalog {
        let providers = vec![ProviderSource::new(
            "openrouter",
            ProviderInfo {
                api_url: "https://openrouter.ai/api/v1".into(),
                env_vars: vec!["OPENROUTER_API_KEY".into()],
                api_type: ProviderType::OpenRouter,
            },
        )];
        let info = ModelInfo {
            modalities: Modality::TEXT,
            max_input: 128_000,
            max_output: 16_384,
            temperature: Some(1.0),
            top_p: Some(0.95),
        };
        let models: Vec<ProviderModelSource<'_>> =
            [("openai/gpt-4.1-mini", info), ("openai/gpt-4o", info)]
                .into_iter()
                .map(|(key, i)| ProviderModelSource::new(ProviderIdx::new(0), key, i))
                .collect();
        ModelCatalog::build(&providers, &models).expect("catalog fixture should build")
    }

    fn credentials() -> Arc<CredentialResolver<false>> {
        let mut resolver = CredentialResolver::without_env();
        resolver.set_override("OPENROUTER_API_KEY", "test-key");
        Arc::new(resolver)
    }

    fn runtime_with_agents(agents: Vec<AgentConfig>) -> AgentRuntimeBuilder {
        AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries(agents))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
    }

    fn build_test_context(
        runtime: llm_coding_tools_agents::AgentRuntime,
    ) -> Arc<TaskBuildContext<CredentialResolver<false>>> {
        Arc::new(TaskBuildContext::new_for_test(
            runtime,
            Arc::new(catalog()),
            credentials(),
        ))
    }

    #[tokio::test]
    async fn validate_target_rejects_unknown_target() {
        let runtime = runtime_with_agents(vec![agent(
            "caller",
            AgentMode::All,
            allow_tools(&[tool_names::TASK]),
        )])
        .build();
        let context = build_test_context(runtime);
        let handle = TaskHandle::new(context, 0);

        let input = TaskInput {
            description: "test".into(),
            prompt: "test prompt".into(),
            subagent_type: "nonexistent".into(),
            session_id: None,
            command: None,
        };

        let result = handle.execute("caller", input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "task");
                assert!(!errors.is_empty());
                let error_message = &errors[0].message;
                assert!(error_message.contains("nonexistent"));
                assert!(error_message.contains("unknown"));
            }
            _ => panic!("Expected ValidationFailed error, got: {:?}", err),
        }
    }

    #[tokio::test]
    async fn validate_target_rejects_primary_target() {
        let runtime = runtime_with_agents(vec![
            agent("caller", AgentMode::All, allow_tools(&[tool_names::TASK])),
            agent("primary-agent", AgentMode::Primary, allow_tools(&[])),
        ])
        .build();
        let context = build_test_context(runtime);
        let handle = TaskHandle::new(context, 0);

        let input = TaskInput {
            description: "test".into(),
            prompt: "test prompt".into(),
            subagent_type: "primary-agent".into(),
            session_id: None,
            command: None,
        };

        let result = handle.execute("caller", input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "task");
                assert!(!errors.is_empty());
                let error_message = &errors[0].message;
                assert!(error_message.contains("primary"));
                assert!(error_message.contains("mode"));
            }
            _ => panic!("Expected ValidationFailed error, got: {:?}", err),
        }
    }

    #[tokio::test]
    async fn validate_target_rejects_permission_denied_target() {
        let runtime = runtime_with_agents(vec![
            agent(
                "caller",
                AgentMode::All,
                pattern_task(&[("*", PermissionAction::Deny)]),
            ),
            agent("target", AgentMode::All, allow_tools(&[])),
        ])
        .build();
        let context = build_test_context(runtime);
        let handle = TaskHandle::new(context, 0);

        let input = TaskInput {
            description: "test".into(),
            prompt: "test prompt".into(),
            subagent_type: "target".into(),
            session_id: None,
            command: None,
        };

        let result = handle.execute("caller", input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "task");
                assert!(!errors.is_empty());
                let error_message = &errors[0].message;
                assert!(error_message.contains("not allowed"));
                assert!(error_message.contains("caller"));
            }
            _ => panic!("Expected ValidationFailed error, got: {:?}", err),
        }
    }

    #[tokio::test]
    async fn execute_rejects_session_id() {
        let runtime = runtime_with_agents(vec![
            agent("caller", AgentMode::All, allow_tools(&[tool_names::TASK])),
            agent("target", AgentMode::All, allow_tools(&[])),
        ])
        .build();
        let context = build_test_context(runtime);
        let handle = TaskHandle::new(context, 0);

        let input = TaskInput {
            description: "test".into(),
            prompt: "test prompt".into(),
            subagent_type: "target".into(),
            session_id: Some("session-123".into()),
            command: None,
        };

        let result = handle.execute("caller", input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "task");
                assert!(!errors.is_empty());
                let error_field = errors[0].field.as_ref().expect("Expected field");
                assert_eq!(error_field, "session_id");
                let error_message = &errors[0].message;
                assert!(error_message.contains("not supported"));
                assert!(error_message.contains("omit"));
            }
            _ => panic!("Expected ValidationFailed error, got: {:?}", err),
        }
    }

    #[tokio::test]
    async fn execute_rejects_calls_at_max_task_depth() {
        // Defense-in-depth: even if the Task tool were somehow present at max depth,
        // execute() rejects the call.
        let runtime = runtime_with_agents(vec![
            agent("caller", AgentMode::All, allow_tools(&[tool_names::TASK])),
            agent("target", AgentMode::All, allow_tools(&[])),
        ])
        .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
        .max_task_depth(0)
        .build();
        let context = build_test_context(runtime);
        let handle = TaskHandle::new(context, 0);

        let input = TaskInput {
            description: "test".into(),
            prompt: "test prompt".into(),
            subagent_type: "target".into(),
            session_id: None,
            command: None,
        };

        let result = handle.execute("caller", input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        match &err {
            ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, "task");
                assert!(!errors.is_empty());
                let error_message = &errors[0].message;
                assert!(error_message.contains("max_task_depth"));
                assert!(error_message.contains("target"));
            }
            _ => panic!("Expected ValidationFailed error, got: {:?}", err),
        }
    }
}
