use super::*;
use llm_coding_tools_agents::{
    PermissionAction, Rule, TaskError as AgentTaskError, TaskOutput as AgentTaskOutput,
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
    ) -> Result<AgentTaskOutput, AgentTaskError> {
        Ok(AgentTaskOutput::new(format!(
            "Executed '{}': {} (tools: {})",
            input.description,
            input.prompt,
            allowed_tools.join(", ")
        )))
    }

    fn all_agents(&self) -> Vec<String> {
        self.agents.iter().map(|(n, _)| n.clone()).collect()
    }

    fn agent_tools(&self, _agent_name: &str) -> Result<Vec<String>, AgentTaskError> {
        Ok(self.tools.clone())
    }

    fn agent_rules(&self, _agent_name: &str) -> Result<Ruleset, AgentTaskError> {
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
