//! Task tool for invoking subagents (rig adapter).
//!
//! Thin wrapper around [`TaskToolCore`] for rig framework compatibility.

use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::{ToolError, ToolOutput};
use llm_coding_tools_subagents::{
    Ruleset, TaskError as SubagentTaskError, TaskInput, TaskRunner, TaskToolCore,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::Arc;

/// Arguments for the Task tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TaskArgs {
    /// A short (3-5 words) description of the task.
    pub description: String,
    /// The task for the agent to perform.
    pub prompt: String,
    /// The type of specialized agent to use for this task.
    pub subagent_type: String,
    /// Existing Task session to continue.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The command that triggered this task.
    #[serde(default)]
    pub command: Option<String>,
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

/// Task tool for rig framework.
///
/// Wraps [`TaskToolCore`] to provide subagent invocation capabilities.
/// Stores deps in struct - does NOT require `Deps: Default`.
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

impl<R: TaskRunner + 'static> Tool for TaskTool<R> {
    const NAME: &'static str = tool_names::TASK;

    type Error = ToolError;
    type Args = TaskArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description: self.core.build_description(),
            parameters: serde_json::to_value(schemars::schema_for!(TaskArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input: TaskInput = args.into();

        let result = self
            .core
            .execute(input, &self.deps)
            .await
            .map_err(|e| match e {
                SubagentTaskError::UnknownAgent(name) => {
                    ToolError::Validation(format!("Unknown agent type: {}", name))
                }
                SubagentTaskError::AccessDenied(name) => ToolError::Validation(format!(
                    "Access denied: cannot invoke subagent '{}'",
                    name
                )),
                SubagentTaskError::NotInvocable(name) => ToolError::Validation(format!(
                    "Subagent '{}' is not available for task invocation",
                    name
                )),
                SubagentTaskError::Execution(msg) => ToolError::Execution(msg),
                SubagentTaskError::Configuration(msg) => ToolError::Validation(msg),
            })?;

        Ok(ToolOutput::new(result.format()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use llm_coding_tools_subagents::{PermissionAction, Rule, TaskOutput as SubagentTaskOutput};

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

    #[tokio::test]
    async fn task_tool_denies_unpermitted_agent() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));
        let rules = Ruleset::new(); // Empty = default deny
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let args = TaskArgs {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "agent-a".to_string(),
            session_id: None,
            command: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Access denied"));
    }

    #[tokio::test]
    async fn task_tool_returns_unknown_for_nonexistent_agent() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));
        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let args = TaskArgs {
            description: "Test".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "nonexistent".to_string(),
            session_id: None,
            command: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown agent type"));
    }

    #[tokio::test]
    async fn task_tool_executes_permitted_task() {
        let runner = Arc::new(MockRunner::new(vec![("agent-a", true)], vec!["Read"]));

        let mut rules = Ruleset::new();
        rules.push(Rule::new("task", "*", PermissionAction::Allow));
        let deps = Arc::new(());

        let tool = TaskTool::new(runner, rules, deps);
        let args = TaskArgs {
            description: "Test task".to_string(),
            prompt: "Do something".to_string(),
            subagent_type: "agent-a".to_string(),
            session_id: None,
            command: None,
        };

        let result = tool.call(args).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("Test task"));
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
}
