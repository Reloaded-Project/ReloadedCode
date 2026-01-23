//! Task tool for invoking subagents (serdesAI adapter).
//!
//! Thin wrapper around [`TaskToolCore`] for serdesAI framework compatibility.
//!
//! **Note:** This adapter stores `deps: Arc<R::Deps>` in the struct, not retrieving
//! from `RunContext`. This is consistent with other serdesAI tools that ignore `_ctx`.

use crate::convert::to_serdes_result;
use async_trait::async_trait;
use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_subagents::{
    Ruleset, TaskError as SubagentTaskError, TaskInput, TaskRunner, TaskToolCore,
};
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::sync::Arc;

/// Arguments for the Task tool (internal deserialization).
#[derive(Debug, Clone, Deserialize)]
struct TaskArgs {
    description: String,
    prompt: String,
    subagent_type: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

impl From<TaskArgs> for TaskInput {
    fn from(args: TaskArgs) -> Self {
        Self {
            description: args.description,
            prompt: args.prompt,
            subagent_type: args.subagent_type,
            session_id: args.session_id,
            command: args.command,
        }
    }
}

/// Task tool for serdesAI framework.
///
/// Wraps [`TaskToolCore`] to provide subagent invocation capabilities.
/// **Stores deps in struct** - does NOT use `ctx.deps` from RunContext.
///
/// # Type Parameters
///
/// * `R` - The [`TaskRunner`] implementation
pub struct TaskTool<R: TaskRunner> {
    core: TaskToolCore<R>,
    deps: Arc<R::Deps>,
}

impl<R: TaskRunner> TaskTool<R> {
    /// Creates a new Task tool with the given runner, caller permissions, and deps.
    pub fn new(runner: Arc<R>, caller_rules: Ruleset, deps: Arc<R::Deps>) -> Self {
        Self {
            core: TaskToolCore::new(runner, caller_rules),
            deps,
        }
    }

    /// Returns the core task tool logic.
    #[inline]
    pub fn core(&self) -> &TaskToolCore<R> {
        &self.core
    }
}

impl<R: TaskRunner> Clone for TaskTool<R> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            deps: Arc::clone(&self.deps),
        }
    }
}

#[async_trait]
impl<R, Deps> Tool<Deps> for TaskTool<R>
where
    R: TaskRunner + 'static,
    Deps: Send + Sync,
{
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(tool_names::TASK, self.core.build_description()).with_parameters(
            SchemaBuilder::new()
                .string_constrained(
                    "description",
                    "A short (3-5 words) description of the task",
                    true,
                    Some(1),
                    Some(100),
                    None,
                )
                .string_constrained(
                    "prompt",
                    "The task for the agent to perform",
                    true,
                    Some(1),
                    None,
                    None,
                )
                .string_constrained(
                    "subagent_type",
                    "The type of specialized agent to use for this task",
                    true,
                    Some(1),
                    None,
                    None,
                )
                .string("session_id", "Existing Task session to continue", false)
                .string("command", "The command that triggered this task", false)
                .build()
                .expect("schema serialization should never fail"),
        )
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: TaskArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::TASK, None, e.to_string()))?;

        let input: TaskInput = args.into();

        // Use self.deps, NOT ctx.deps (consistent with other serdesAI tools)
        let result = self
            .core
            .execute(input, &self.deps)
            .await
            .map_err(|e| match e {
                SubagentTaskError::UnknownAgent(name) => ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Unknown agent type: {}", name),
                ),
                SubagentTaskError::AccessDenied(name) => ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Access denied: cannot invoke subagent '{}'", name),
                ),
                SubagentTaskError::NotInvocable(name) => ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Subagent '{}' is not available for task invocation", name),
                ),
                SubagentTaskError::Execution(msg) => ToolError::execution_failed(msg),
                SubagentTaskError::Configuration(msg) => {
                    ToolError::validation_error(tool_names::TASK, None, msg)
                }
            })?;

        to_serdes_result(
            tool_names::TASK,
            Ok(llm_coding_tools_core::ToolOutput::new(result.format())),
        )
    }
}

impl<R: TaskRunner> ToolContext for TaskTool<R> {
    const NAME: &'static str = tool_names::TASK;

    fn context(&self) -> &'static str {
        "Use the Task tool to delegate complex, multi-step tasks to specialized subagents."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llm_coding_tools_subagents::{
        PermissionAction, Rule, TaskError as SubagentTaskError, TaskOutput as SubagentTaskOutput,
    };

    /// Mock runner for testing
    struct MockRunner {
        agents: Vec<(String, bool)>,
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
        ) -> Result<SubagentTaskOutput, SubagentTaskError> {
            Ok(SubagentTaskOutput::new(format!(
                "Executed '{}': {} (tools: {})",
                input.description,
                input.prompt,
                allowed_tools.join(", ")
            )))
        }

        fn all_agents(&self) -> Vec<String> {
            self.agents.iter().map(|(n, _)| n.clone()).collect()
        }

        fn agent_tools(&self, _agent_name: &str) -> Result<Vec<String>, SubagentTaskError> {
            Ok(self.tools.clone())
        }

        fn agent_rules(&self, _agent_name: &str) -> Result<Ruleset, SubagentTaskError> {
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

    fn mock_ctx() -> RunContext<()> {
        RunContext::minimal("test-model")
    }

    #[tokio::test]
    async fn task_tool_denies_unpermitted_agent() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));
        let rules = Ruleset::new(); // Empty = default deny
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let args = serde_json::json!({
            "description": "Test",
            "prompt": "Do something",
            "subagent_type": "agent-a"
        });

        let result = tool.call(&mock_ctx(), args).await;
        assert!(result.is_err());
        // Check error contains Access denied message
        match result.unwrap_err() {
            serdes_ai::tools::ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, tool_names::TASK);
                assert!(errors[0].message.contains("Access denied"));
            }
            _ => panic!("Expected ValidationFailed error"),
        }
    }

    #[tokio::test]
    async fn task_tool_returns_unknown_for_nonexistent_agent() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));
        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let args = serde_json::json!({
            "description": "Test",
            "prompt": "Do something",
            "subagent_type": "nonexistent"
        });

        let result = tool.call(&mock_ctx(), args).await;
        assert!(result.is_err());
        // Check error contains Unknown agent type message
        match result.unwrap_err() {
            serdes_ai::tools::ToolError::ValidationFailed { tool_name, errors } => {
                assert_eq!(tool_name, tool_names::TASK);
                assert!(errors[0].message.contains("Unknown agent type"));
            }
            _ => panic!("Expected ValidationFailed error"),
        }
    }

    #[tokio::test]
    async fn task_tool_executes_permitted_task() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let args = serde_json::json!({
            "description": "Test task",
            "prompt": "Do something",
            "subagent_type": "agent-a"
        });

        let result = tool.call(&mock_ctx(), args).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.as_text().unwrap().contains("Test task"));
    }

    #[test]
    fn task_tool_description_includes_agents() {
        let runner = Arc::new(MockRunner::new(
            vec![("search", true), ("fetch", true)],
            vec!["Read", "Glob"],
        ));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let description = tool.core.build_description();

        assert!(description.contains("search"));
        assert!(description.contains("fetch"));
    }

    #[test]
    fn task_tool_schema_has_required_fields() {
        let runner = Arc::new(MockRunner::new(vec![("agent", true)], vec!["Read"]));
        let rules = Ruleset::new();
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let def = serdes_ai::tools::Tool::<()>::definition(&tool);

        assert_eq!(def.name(), tool_names::TASK);

        let params = def.parameters();
        assert_eq!(params["type"], "object");

        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&serde_json::json!("description")));
        assert!(required.contains(&serde_json::json!("prompt")));
        assert!(required.contains(&serde_json::json!("subagent_type")));
    }
}
