use super::*;
use async_trait::async_trait;
use llm_coding_tools_agents::{AgentConfig, AgentMode, PermissionAction, Rule, Ruleset};
use serdes_ai::tools::RunContext;
use std::sync::{Arc, Mutex};

use crate::registry::{AgentRegistry, AgentRegistryEntry, RegistryAgent, RegistryAgentError};

struct MockAgent {
    last_prompt: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl RegistryAgent<()> for MockAgent {
    async fn prompt(&self, message: String, _deps: Arc<()>) -> Result<String, RegistryAgentError> {
        *self.last_prompt.lock().unwrap() = Some(message);
        Ok("mock response".to_string())
    }
}

fn make_entry(name: &str, mode: AgentMode, hidden: bool) -> AgentRegistryEntry<MockAgent> {
    AgentRegistryEntry {
        config: AgentConfig {
            name: name.to_string(),
            mode,
            description: String::new(),
            model: None,
            hidden,
            temperature: None,
            top_p: None,
            permission: indexmap::IndexMap::new(),
            options: std::collections::HashMap::new(),
            prompt: String::new(),
        },
        tool_names: vec!["Read".to_string(), "Bash".to_string()],
        system_prompt: String::new(),
        agent: MockAgent {
            last_prompt: Arc::new(Mutex::new(None)),
        },
    }
}

#[tokio::test]
async fn task_tool_hidden_flag_is_noop_for_description() {
    let registry = AgentRegistry::from_entries([
        (
            "visible".to_string(),
            make_entry("visible", AgentMode::Subagent, false),
        ),
        (
            "hidden".to_string(),
            make_entry("hidden", AgentMode::Subagent, true),
        ),
    ]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    // Need type annotation: RuntimeDeps = ()
    let defn = <TaskTool<MockAgent, ()> as serdes_ai::tools::Tool<()>>::definition(&tool);
    let description = defn.description();
    assert!(description.contains("visible"));
    assert!(description.contains("hidden"));
}

#[tokio::test]
async fn task_tool_denies_unpermitted_agent() {
    let registry = AgentRegistry::from_entries([(
        "agent-a".to_string(),
        make_entry("agent-a", AgentMode::Subagent, false),
    )]);
    let rules = Ruleset::new();
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let args = serde_json::json!({
        "description": "Test",
        "prompt": "Do something",
        "subagent_type": "agent-a"
    });

    let result = tool.call(&RunContext::minimal("test-model"), args).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        serdes_ai::tools::ToolError::ValidationFailed { tool_name, errors } => {
            assert_eq!(tool_name, tool_names::TASK);
            assert!(errors[0].message.contains("Access denied"));
        }
        _ => panic!("Expected ValidationFailed error"),
    }
}

#[tokio::test]
async fn task_tool_rejects_non_invocable_agent() {
    let registry = AgentRegistry::from_entries([(
        "primary-only".to_string(),
        make_entry("primary-only", AgentMode::Primary, false),
    )]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let args = serde_json::json!({
        "description": "Test",
        "prompt": "Do something",
        "subagent_type": "primary-only"
    });

    let result = tool.call(&RunContext::minimal("test-model"), args).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        serdes_ai::tools::ToolError::ValidationFailed { errors, .. } => {
            assert!(errors[0].message.contains("not available"));
        }
        _ => panic!("Expected ValidationFailed error"),
    }
}

#[tokio::test]
async fn task_tool_validates_and_builds_task_message() {
    let entry = make_entry("agent-a", AgentMode::Subagent, false);
    let last_prompt = entry.agent.last_prompt.clone();
    let registry = AgentRegistry::from_entries([("agent-a".to_string(), entry)]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let args = serde_json::json!({
        "description": "Test task",
        "prompt": "Do something",
        "subagent_type": "agent-a",
        "session_id": "sess-1",
        "command": "/cmd"
    });

    let _ = tool
        .call(&RunContext::minimal("test-model"), args)
        .await
        .unwrap();
    let message = last_prompt.lock().unwrap().clone().unwrap();
    assert!(message.contains("<task_context>"));
    assert!(message.contains("description: Test task"));
    assert!(message.contains("command: /cmd"));
    assert!(message.contains("session_id: sess-1"));
    assert!(message.contains("<task_prompt>"));
    assert!(message.contains("Do something</task_prompt>"));
}

#[tokio::test]
async fn task_tool_description_filters_by_permissions() {
    let registry = AgentRegistry::from_entries([
        (
            "allowed".to_string(),
            make_entry("allowed", AgentMode::Subagent, false),
        ),
        (
            "denied".to_string(),
            make_entry("denied", AgentMode::Subagent, false),
        ),
    ]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "allowed", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    // Need type annotation: RuntimeDeps = ()
    let defn = <TaskTool<MockAgent, ()> as serdes_ai::tools::Tool<()>>::definition(&tool);
    let description = defn.description();
    assert!(description.contains("allowed"));
    assert!(!description.contains("denied"));
}

#[tokio::test]
async fn task_tool_hidden_agent_remains_invocable() {
    let entry = make_entry("hidden", AgentMode::Subagent, true);
    let last_prompt = entry.agent.last_prompt.clone();
    let registry = AgentRegistry::from_entries([("hidden".to_string(), entry)]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "hidden", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let args = serde_json::json!({
        "description": "Hidden run",
        "prompt": "Do hidden",
        "subagent_type": "hidden"
    });

    let _ = tool
        .call(&RunContext::minimal("test-model"), args)
        .await
        .unwrap();
    assert!(last_prompt.lock().unwrap().is_some());
}

#[tokio::test]
async fn task_tool_returns_unknown_for_nonexistent_agent() {
    let registry = AgentRegistry::from_entries([(
        "agent-a".to_string(),
        make_entry("agent-a", AgentMode::Subagent, false),
    )]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let args = serde_json::json!({
        "description": "Test",
        "prompt": "Do something",
        "subagent_type": "nonexistent"
    });

    let result = tool.call(&RunContext::minimal("test-model"), args).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        serdes_ai::tools::ToolError::ValidationFailed { tool_name, errors } => {
            assert_eq!(tool_name, tool_names::TASK);
            assert!(errors[0].message.contains("Unknown agent type"));
        }
        _ => panic!("Expected ValidationFailed error"),
    }
}

#[test]
fn task_tool_schema_has_required_fields() {
    let registry = AgentRegistry::from_entries([(
        "agent".to_string(),
        make_entry("agent", AgentMode::Subagent, false),
    )]);
    let rules = Ruleset::new();
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let binding = <TaskTool<MockAgent, ()> as serdes_ai::tools::Tool<()>>::definition(&tool);

    assert_eq!(binding.name(), tool_names::TASK);

    let params = binding.parameters();
    assert_eq!(params["type"], "object");

    let required = params["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("description")));
    assert!(required.contains(&serde_json::json!("prompt")));
    assert!(required.contains(&serde_json::json!("subagent_type")));
}
