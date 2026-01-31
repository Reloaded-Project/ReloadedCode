use super::*;
use async_trait::async_trait;
use llm_coding_tools_agents::{PermissionAction, Rule, TaskOutput as AgentTaskOutput};

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
