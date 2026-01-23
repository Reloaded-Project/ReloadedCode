//! Task tool types, runner abstraction, and core logic.
//!
//! Provides the core types and trait for executing tasks with subagents.
//! Framework-specific adapters (rig, serdesAI) wrap [`TaskToolCore`].

use crate::permission::Ruleset;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

/// Input for task execution.
#[derive(Debug, Clone)]
pub struct TaskInput {
    /// Short description (3-5 words) of the task.
    pub description: String,
    /// The prompt/task for the subagent to perform.
    pub prompt: String,
    /// The subagent type/name to invoke.
    pub subagent_type: String,
    /// Optional session ID to continue an existing task session.
    pub session_id: Option<String>,
    /// Optional command that triggered this task (for context).
    pub command: Option<String>,
}

/// Output from task execution.
#[derive(Debug, Clone)]
pub struct TaskOutput {
    /// The text summary/response from the subagent.
    pub summary: String,
    /// Session ID for continuation (if supported by implementation).
    pub session_id: Option<String>,
    /// Optional metadata from the execution.
    pub metadata: Option<serde_json::Value>,
}

impl TaskOutput {
    /// Creates a new task output with just a summary.
    #[inline]
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            session_id: None,
            metadata: None,
        }
    }

    /// Sets the session ID.
    #[inline]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Sets metadata.
    #[inline]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Formats the output for LLM consumption.
    pub fn format(&self) -> String {
        let mut content = self.summary.clone();

        if let Some(ref session_id) = self.session_id {
            content.push_str("\n\n<task_metadata>\n");
            content.push_str(&format!("session_id: {}\n", session_id));
            content.push_str("</task_metadata>");
        }

        content
    }
}

/// Errors that can occur during task execution.
#[derive(Debug, Error)]
pub enum TaskError {
    /// The requested subagent type was not found in the registry.
    #[error("unknown subagent type: {0}")]
    UnknownAgent(String),

    /// The caller does not have permission to invoke this subagent.
    #[error("access denied: caller cannot invoke subagent '{0}'")]
    AccessDenied(String),

    /// The subagent is not available for task invocation (e.g., primary-only mode).
    #[error("subagent '{0}' is not available for task invocation")]
    NotInvocable(String),

    /// Task execution failed.
    #[error("execution failed: {0}")]
    Execution(String),

    /// Configuration or setup error.
    #[error("configuration error: {0}")]
    Configuration(String),
}

/// Trait for executing tasks with subagents.
///
/// Implementations are responsible for:
/// 1. Resolving the subagent configuration by name
/// 2. Building the subagent with only the `allowed_tools`
/// 3. Executing the prompt and returning a summary
///
/// **Note:** Access validation (permission checks) is handled by [`TaskToolCore`],
/// not the runner. Runners can assume the caller has permission to invoke the subagent.
///
/// # serdesAI Implementation Note
///
/// When implementing for serdesAI, use `AgentBuilderExt::tool` to register tools,
/// filtering by `allowed_tools`:
///
/// ```ignore
/// let mut builder = AgentBuilder::<Deps, String>::from_model(&config.model)?;
/// for tool in all_tools {
///     if allowed_tools.contains(&tool.name()) {
///         builder = builder.tool(tool);
///     }
/// }
/// ```
#[async_trait]
pub trait TaskRunner: Send + Sync {
    /// The dependencies type for this runner.
    type Deps: Send + Sync;

    /// Executes a task with the specified subagent.
    ///
    /// Called after access validation has passed. The runner should:
    /// 1. Resolve the subagent configuration
    /// 2. Build the subagent with only the allowed tools
    /// 3. Execute the prompt
    ///
    /// # Arguments
    ///
    /// * `input` - The task input (description, prompt, subagent_type, etc.)
    /// * `deps` - The dependencies for the runner
    /// * `allowed_tools` - Tool names the subagent is permitted to use (already filtered)
    ///
    /// # Errors
    ///
    /// Returns [`TaskError`] if:
    /// - The subagent type is not found
    /// - Execution fails
    async fn run(
        &self,
        input: TaskInput,
        deps: &Self::Deps,
        allowed_tools: &[String],
    ) -> Result<TaskOutput, TaskError>;

    /// Returns all registered subagent names (unfiltered).
    ///
    /// Used by [`TaskToolCore`] to check agent existence and filter by caller permissions.
    fn all_agents(&self) -> Vec<String>;

    /// Returns the tool names available to a specific subagent (before filtering).
    ///
    /// Used to build the tool description and compute allowed tools.
    fn agent_tools(&self, agent_name: &str) -> Result<Vec<String>, TaskError>;

    /// Returns the permission rules for a specific subagent.
    ///
    /// Used by [`TaskToolCore`] to compute which tools the subagent can use.
    fn agent_rules(&self, agent_name: &str) -> Result<Ruleset, TaskError>;

    /// Checks if an agent is invocable (not primary-only).
    fn is_invocable(&self, agent_name: &str) -> bool;
}

/// Task tool description template.
/// `{agents}` is replaced with the list of available subagents.
const DESCRIPTION_TEMPLATE: &str = r#"Launch a new agent to handle complex, multistep tasks autonomously.

Available agent types and the tools they have access to:
{agents}

When using the Task tool, you must specify a subagent_type parameter to select which agent type to use."#;

/// Core Task tool logic with enforced access validation.
///
/// Wraps a [`TaskRunner`] and ensures access checks are ALWAYS performed
/// before execution. Framework adapters delegate to this core.
///
/// # Type Parameters
///
/// * `R` - The [`TaskRunner`] implementation
pub struct TaskToolCore<R: TaskRunner> {
    runner: Arc<R>,
    caller_rules: Ruleset,
}

impl<R: TaskRunner> TaskToolCore<R> {
    /// Creates a new TaskToolCore with the given runner and caller permissions.
    pub fn new(runner: Arc<R>, caller_rules: Ruleset) -> Self {
        Self {
            runner,
            caller_rules,
        }
    }

    /// Returns the runner reference.
    #[inline]
    pub fn runner(&self) -> &R {
        &self.runner
    }

    /// Returns the caller's permission rules.
    #[inline]
    pub fn caller_rules(&self) -> &Ruleset {
        &self.caller_rules
    }

    /// Checks if an agent exists in the registry.
    fn agent_exists(&self, name: &str) -> bool {
        self.runner.all_agents().iter().any(|n| n == name)
    }

    /// Returns the list of accessible subagent names for the caller.
    ///
    /// Filters all agents by:
    /// 1. Invocability (not primary-only)
    /// 2. Caller's `task` permission rules
    pub fn accessible_agents(&self) -> Vec<String> {
        self.runner
            .all_agents()
            .into_iter()
            .filter(|name| {
                self.runner.is_invocable(name) && self.caller_rules.is_allowed("task", name)
            })
            .collect()
    }

    /// Computes the allowed tools for a subagent.
    ///
    /// Takes the subagent's available tools and filters by its permission rules.
    /// Normalizes tool names to lowercase for comparison but preserves original casing.
    fn compute_allowed_tools(&self, agent_name: &str) -> Result<Vec<String>, TaskError> {
        let available_tools = self.runner.agent_tools(agent_name)?;
        let agent_rules = self.runner.agent_rules(agent_name)?;

        // Filter tools: normalize for comparison, preserve original casing
        let allowed: Vec<String> = available_tools
            .into_iter()
            .filter(|name| agent_rules.is_allowed(&name.to_ascii_lowercase(), "*"))
            .collect();

        Ok(allowed)
    }

    /// Builds the tool description with available subagents and their tools.
    pub fn build_description(&self) -> String {
        let accessible = self.accessible_agents();

        if accessible.is_empty() {
            return "Task tool is not available - no accessible subagents.".to_string();
        }

        let agents_list: String = accessible
            .iter()
            .filter_map(|name| {
                self.compute_allowed_tools(name)
                    .ok()
                    .map(|tools| format!("- {}: {}", name, tools.join(", ")))
            })
            .collect::<Vec<_>>()
            .join("\n");

        DESCRIPTION_TEMPLATE.replace("{agents}", &agents_list)
    }

    /// Executes a task with enforced access validation.
    ///
    /// This method ALWAYS validates in order:
    /// 1. The agent exists (returns UnknownAgent if not)
    /// 2. The subagent is invocable (returns NotInvocable if not)
    /// 3. The caller has `task` permission for the requested subagent (returns AccessDenied if not)
    ///
    /// Then computes allowed tools and delegates to the runner.
    ///
    /// # Errors
    ///
    /// Returns [`TaskError::UnknownAgent`] if the agent doesn't exist.
    /// Returns [`TaskError::NotInvocable`] if the agent is primary-only.
    /// Returns [`TaskError::AccessDenied`] if the caller lacks permission.
    pub async fn execute(&self, input: TaskInput, deps: &R::Deps) -> Result<TaskOutput, TaskError> {
        // 1. Check agent existence FIRST
        if !self.agent_exists(&input.subagent_type) {
            return Err(TaskError::UnknownAgent(input.subagent_type));
        }

        // 2. Check invocability (is it a subagent, not primary-only?)
        if !self.runner.is_invocable(&input.subagent_type) {
            return Err(TaskError::NotInvocable(input.subagent_type));
        }

        // 3. Enforce access validation
        if !self.caller_rules.is_allowed("task", &input.subagent_type) {
            return Err(TaskError::AccessDenied(input.subagent_type));
        }

        // 4. Compute allowed tools for the subagent
        let allowed_tools = self.compute_allowed_tools(&input.subagent_type)?;

        // 5. Delegate to runner with allowed tools
        self.runner.run(input, deps, &allowed_tools).await
    }
}

impl<R: TaskRunner> Clone for TaskToolCore<R> {
    fn clone(&self) -> Self {
        Self {
            runner: Arc::clone(&self.runner),
            caller_rules: self.caller_rules.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionAction;
    use crate::permission::Rule;

    /// Mock runner for testing
    struct MockRunner {
        agents: Vec<(String, bool)>, // (name, invocable)
        tools: Vec<String>,
    }

    impl MockRunner {
        fn new(agents: Vec<(&str, bool)>, tools: Vec<&str>) -> Self {
            Self {
                agents: agents
                    .into_iter()
                    .map(|(n, i)| (n.to_string(), i))
                    .collect(),
                tools: tools.into_iter().map(String::from).collect(),
            }
        }
    }

    #[async_trait]
    impl TaskRunner for MockRunner {
        type Deps = ();

        async fn run(
            &self,
            input: TaskInput,
            _deps: &(),
            allowed_tools: &[String],
        ) -> Result<TaskOutput, TaskError> {
            Ok(TaskOutput::new(format!(
                "Executed '{}': {} (tools: {})",
                input.description,
                input.prompt,
                allowed_tools.join(", ")
            )))
        }

        fn all_agents(&self) -> Vec<String> {
            self.agents.iter().map(|(n, _)| n.clone()).collect()
        }

        fn agent_tools(&self, _agent_name: &str) -> Result<Vec<String>, TaskError> {
            Ok(self.tools.clone())
        }

        fn agent_rules(&self, _agent_name: &str) -> Result<Ruleset, TaskError> {
            // Allow all tools by default in mock
            let mut rules = Ruleset::new();
            for tool in &self.tools {
                rules.push(Rule::new(tool, "*", PermissionAction::Allow));
            }
            Ok(rules)
        }

        fn is_invocable(&self, agent_name: &str) -> bool {
            self.agents
                .iter()
                .find(|(n, _)| n == agent_name)
                .map(|(_, i)| *i)
                .unwrap_or(false)
        }
    }

    #[test]
    fn accessible_agents_filters_by_permission_and_invocability() {
        let runner = Arc::new(MockRunner::new(
            vec![
                ("agent-a", true),
                ("agent-b", true),
                ("primary-only", false),
            ],
            vec!["Read"],
        ));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "agent-a", PermissionAction::Allow));
        // agent-b not allowed, primary-only not invocable

        let core = TaskToolCore::new(runner, rules);
        let accessible = core.accessible_agents();

        assert_eq!(accessible, vec!["agent-a"]);
    }

    #[tokio::test]
    async fn execute_returns_unknown_agent_for_nonexistent() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));
        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));

        let core = TaskToolCore::new(runner, rules);
        let input = TaskInput {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "nonexistent".to_string(),
            session_id: None,
            command: None,
        };

        let result = core.execute(input, &()).await;
        assert!(matches!(result, Err(TaskError::UnknownAgent(name)) if name == "nonexistent"));
    }

    #[tokio::test]
    async fn execute_returns_not_invocable_before_access_denied() {
        let runner = Arc::new(MockRunner::new(vec![("primary-only", false)], vec!["Read"]));
        let rules = Ruleset::new(); // Empty = would deny access

        let core = TaskToolCore::new(runner, rules);
        let input = TaskInput {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "primary-only".to_string(),
            session_id: None,
            command: None,
        };

        let result = core.execute(input, &()).await;
        // Should be NotInvocable, not AccessDenied (checked first after existence)
        assert!(matches!(result, Err(TaskError::NotInvocable(_))));
    }

    #[tokio::test]
    async fn execute_enforces_access_validation() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));
        let rules = Ruleset::new(); // Empty = default deny

        let core = TaskToolCore::new(runner, rules);
        let input = TaskInput {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "agent-a".to_string(),
            session_id: None,
            command: None,
        };

        let result = core.execute(input, &()).await;
        assert!(matches!(result, Err(TaskError::AccessDenied(_))));
    }

    #[tokio::test]
    async fn execute_passes_allowed_tools_to_runner() {
        let runner = Arc::new(MockRunner::new(
            vec![("agent-a", true)],
            vec!["Read", "Write", "Bash"],
        ));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));

        let core = TaskToolCore::new(runner, rules);
        let input = TaskInput {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "agent-a".to_string(),
            session_id: None,
            command: None,
        };

        let result = core.execute(input, &()).await.unwrap();
        // MockRunner's agent_rules allows all tools, so all should be passed
        assert!(result.summary.contains("Read"));
        assert!(result.summary.contains("Write"));
        assert!(result.summary.contains("Bash"));
    }

    #[tokio::test]
    async fn execute_succeeds_with_valid_access() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "agent-a", PermissionAction::Allow));

        let core = TaskToolCore::new(runner, rules);
        let input = TaskInput {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "agent-a".to_string(),
            session_id: None,
            command: None,
        };

        let result = core.execute(input, &()).await;
        assert!(result.is_ok());
    }

    #[test]
    fn build_description_includes_accessible_agents() {
        let runner = Arc::new(MockRunner::new(
            vec![("search", true), ("fetch", true)],
            vec!["Read", "Glob"],
        ));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));

        let core = TaskToolCore::new(runner, rules);
        let description = core.build_description();

        assert!(description.contains("search"));
        assert!(description.contains("fetch"));
        assert!(description.contains("Read"));
        assert!(description.contains("Glob"));
    }

    #[test]
    fn task_output_format_includes_session_id() {
        let output = TaskOutput::new("Result").with_session_id("sess-123");
        let formatted = output.format();

        assert!(formatted.contains("Result"));
        assert!(formatted.contains("session_id: sess-123"));
    }
}
