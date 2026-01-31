use super::*;
use async_trait::async_trait;
use llm_coding_tools_agents::{AgentConfig, AgentMode, PermissionAction, Rule, Ruleset};
use std::sync::{Arc, Mutex};

use crate::registry::{AgentRegistry, AgentRegistryEntry, RegistryAgent, RegistryAgentError};

struct MockAgent {
    last_prompt: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl RegistryAgent for MockAgent {
    async fn prompt(&self, message: String) -> Result<String, RegistryAgentError> {
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
            permission: Default::default(),
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
async fn task_tool_denies_unpermitted_agent() {
    let registry = AgentRegistry::from_entries([(
        "agent-a".to_string(),
        make_entry("agent-a", AgentMode::Subagent, false),
    )]);
    let rules = Ruleset::new();
    let tool = TaskTool::new(Arc::new(registry), rules);

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
    let registry: AgentRegistry<MockAgent> = AgentRegistry::from_entries([]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules);

    let args = TaskArgs {
        description: "Test".to_string(),
        prompt: "Do something".to_string(),
        subagent_type: "missing".to_string(),
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
async fn task_tool_rejects_primary_only() {
    let registry = AgentRegistry::from_entries([(
        "primary-only".to_string(),
        make_entry("primary-only", AgentMode::Primary, false),
    )]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules);

    let args = TaskArgs {
        description: "Test".to_string(),
        prompt: "Do something".to_string(),
        subagent_type: "primary-only".to_string(),
        session_id: None,
        command: None,
    };

    let result = tool.call(args).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not available"));
}

#[tokio::test]
async fn task_tool_omits_hidden_agents_in_description() {
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
    let tool = TaskTool::new(Arc::new(registry), rules);

    let description = tool.definition("".to_string()).await.description;
    assert!(description.contains("visible"));
    assert!(!description.contains("hidden"));
}

#[tokio::test]
async fn task_tool_description_lists_tools() {
    let registry = AgentRegistry::from_entries([(
        "agent-a".to_string(),
        make_entry("agent-a", AgentMode::Subagent, false),
    )]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules);

    let description = tool.definition("".to_string()).await.description;
    assert!(description.contains("agent-a"));
    assert!(description.contains("Read"));
    assert!(description.contains("Bash"));
}

#[tokio::test]
async fn task_tool_builds_context_message() {
    let entry = make_entry("agent-a", AgentMode::Subagent, false);
    let last_prompt = entry.agent.last_prompt.clone();
    let registry = AgentRegistry::from_entries([("agent-a".to_string(), entry)]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules);

    let args = TaskArgs {
        description: "Test task".to_string(),
        prompt: "Do something".to_string(),
        subagent_type: "agent-a".to_string(),
        session_id: Some("sess-1".to_string()),
        command: Some("/cmd".to_string()),
    };

    let _ = tool.call(args).await.unwrap();
    let message = last_prompt.lock().unwrap().clone().unwrap();
    assert!(message.contains("<task_context>"));
    assert!(message.contains("description: Test task"));
    assert!(message.contains("command: /cmd"));
    assert!(message.contains("session_id: sess-1"));
    assert!(message.contains("<task_prompt>"));
    assert!(message.contains("Do something"));
}

#[tokio::test]
async fn task_tool_invokes_hidden_agent() {
    let entry = make_entry("hidden", AgentMode::Subagent, true);
    let last_prompt = entry.agent.last_prompt.clone();
    let registry = AgentRegistry::from_entries([("hidden".to_string(), entry)]);
    let mut rules = Ruleset::new();
    rules.push(Rule::new("task", "hidden", PermissionAction::Allow));
    let tool = TaskTool::new(Arc::new(registry), rules);

    let args = TaskArgs {
        description: "Hidden run".to_string(),
        prompt: "Do hidden".to_string(),
        subagent_type: "hidden".to_string(),
        session_id: None,
        command: None,
    };

    let _ = tool.call(args).await.unwrap();
    assert!(last_prompt.lock().unwrap().is_some());
}
