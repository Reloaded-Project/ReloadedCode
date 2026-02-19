use super::*;
use async_trait::async_trait;
use llm_coding_tools_agents::{AgentConfig, AgentMode};
use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};
use serdes_ai::tools::RunContext;
use ahash::AHashMap;
use std::sync::{Arc, Mutex};

use crate::registry::{AgentRegistry, AgentRegistryEntry, RegistryAgent, RegistryAgentError};

/// Asserts that a ToolError is a ValidationFailed error with the expected tool name and message fragment.
fn assert_validation_message(err: serdes_ai::tools::ToolError, expected_fragment: &str) {
    match err {
        serdes_ai::tools::ToolError::ValidationFailed { tool_name, errors } => {
            assert_eq!(tool_name, tool_names::TASK);
            assert!(!errors.is_empty());
            assert!(
                errors[0].message.contains(expected_fragment),
                "Expected error message to contain '{}', but got: '{}'",
                expected_fragment,
                errors[0].message
            );
        }
        other => panic!("Expected ValidationFailed error, got: {other:?}"),
    }
}

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
            options: AHashMap::new(),
            prompt: String::new(),
        },
        ruleset: Ruleset::new(),
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

#[tokio::test]
async fn task_tool_description_filters_by_mode_and_permission() {
    let registry = AgentRegistry::from_entries([
        (
            "subagent-allowed".to_string(),
            make_entry("subagent-allowed", AgentMode::Subagent, false),
        ),
        (
            "all-allowed".to_string(),
            make_entry("all-allowed", AgentMode::All, false),
        ),
        (
            "primary-allowed".to_string(),
            make_entry("primary-allowed", AgentMode::Primary, false),
        ),
        (
            "subagent-denied".to_string(),
            make_entry("subagent-denied", AgentMode::Subagent, false),
        ),
    ]);
    let mut rules = Ruleset::new();
    // Allow wildcard first, then deny specific - tests that mode filtering takes precedence over permission
    rules.push(Rule::new("task", "*", PermissionAction::Allow));
    rules.push(Rule::new("task", "subagent-denied", PermissionAction::Deny));
    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    let defn = <TaskTool<MockAgent, ()> as serdes_ai::tools::Tool<()>>::definition(&tool);
    let description = defn.description();

    // Mode-invocable + permission-allowed targets appear
    assert!(description.contains("subagent-allowed"));
    assert!(description.contains("all-allowed"));
    // Primary agents are excluded even if permission-allowed (mode filtering takes precedence)
    assert!(!description.contains("primary-allowed"));
    // Permission-denied agents are excluded even if mode-invocable
    assert!(!description.contains("subagent-denied"));
}

#[tokio::test]
async fn task_tool_invocation_respects_wildcard_last_match_wins() {
    // Create entries with captured prompts for verification
    let approved = make_entry("ops-approved", AgentMode::Subagent, false);
    let approved_prompt = approved.agent.last_prompt.clone();
    let blocked = make_entry("ops-blocked", AgentMode::Subagent, false);
    // Wildcard-only target with no exact rule match - proves wildcard matching is exercised
    let wildcard_only = make_entry("ops-generic", AgentMode::Subagent, false);
    let wildcard_prompt = wildcard_only.agent.last_prompt.clone();

    let registry = AgentRegistry::from_entries([
        ("ops-approved".to_string(), approved),
        ("ops-blocked".to_string(), blocked),
        ("ops-generic".to_string(), wildcard_only),
    ]);

    let mut rules = Ruleset::new();
    // Default deny all via wildcard (must come first to allow later overrides)
    rules.push(Rule::new("task", "*", PermissionAction::Deny));
    // Allow all ops-* via wildcard (overrides the default deny for ops agents)
    rules.push(Rule::new("task", "ops-*", PermissionAction::Allow));
    // Deny specific target (last-match-wins over earlier wildcard allow)
    rules.push(Rule::new("task", "ops-blocked", PermissionAction::Deny));
    // Re-allow specific target (last-match-wins over earlier specific deny)
    rules.push(Rule::new("task", "ops-approved", PermissionAction::Allow));

    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    // Test: ops-blocked should be denied (last match is explicit deny after wildcard allow)
    let denied = serde_json::json!({"description":"d","prompt":"p","subagent_type":"ops-blocked"});
    let denied_result = tool.call(&RunContext::minimal("test-model"), denied).await;
    assert_validation_message(denied_result.unwrap_err(), "Access denied");

    // Test: ops-generic should be allowed (matches wildcard "ops-*", no later matching rule)
    let wildcard =
        serde_json::json!({"description":"d","prompt":"p","subagent_type":"ops-generic"});
    let wildcard_result = tool
        .call(&RunContext::minimal("test-model"), wildcard)
        .await;
    assert!(wildcard_result.is_ok());
    assert!(wildcard_prompt.lock().unwrap().is_some());

    // Test: ops-approved should be allowed (last match is explicit allow after specific deny)
    let allowed =
        serde_json::json!({"description":"d","prompt":"p","subagent_type":"ops-approved"});
    let allowed_result = tool.call(&RunContext::minimal("test-model"), allowed).await;
    assert!(allowed_result.is_ok());
    assert!(approved_prompt.lock().unwrap().is_some());
}

#[tokio::test]
async fn task_tool_invocation_outcome_matrix() {
    // Test matrix covering all four required outcomes: unknown, primary, denied, allowed
    let allowed = make_entry("allowed-agent", AgentMode::Subagent, false);
    let allowed_prompt = allowed.agent.last_prompt.clone();
    let registry = AgentRegistry::from_entries([
        (
            "primary-agent".to_string(),
            make_entry("primary-agent", AgentMode::Primary, false),
        ),
        (
            "denied-agent".to_string(),
            make_entry("denied-agent", AgentMode::Subagent, false),
        ),
        ("allowed-agent".to_string(), allowed),
    ]);

    let mut rules = Ruleset::new();
    // Default deny via wildcard
    rules.push(Rule::new("task", "*", PermissionAction::Deny));
    // Explicit allow for one specific agent
    rules.push(Rule::new("task", "allowed-agent", PermissionAction::Allow));

    let tool = TaskTool::new(Arc::new(registry), rules, Arc::new(()));

    // Outcome 1: Unknown target -> validation error (REQ-010)
    let unknown =
        serde_json::json!({"description":"d","prompt":"p","subagent_type":"missing-agent"});
    let unknown_result = tool.call(&RunContext::minimal("test-model"), unknown).await;
    assert_validation_message(unknown_result.unwrap_err(), "Unknown agent type");

    // Outcome 2: Primary target -> validation error for non-invocable (REQ-011)
    let primary =
        serde_json::json!({"description":"d","prompt":"p","subagent_type":"primary-agent"});
    let primary_result = tool.call(&RunContext::minimal("test-model"), primary).await;
    assert_validation_message(
        primary_result.unwrap_err(),
        "not available for task invocation",
    );

    // Outcome 3: Denied target -> permission-failure outcome (REQ-012)
    let denied = serde_json::json!({"description":"d","prompt":"p","subagent_type":"denied-agent"});
    let denied_result = tool.call(&RunContext::minimal("test-model"), denied).await;
    assert_validation_message(denied_result.unwrap_err(), "Access denied");

    // Outcome 4: Allowed target -> successful dispatch (REQ-013)
    let permitted =
        serde_json::json!({"description":"d","prompt":"p","subagent_type":"allowed-agent"});
    let permitted_result = tool
        .call(&RunContext::minimal("test-model"), permitted)
        .await;
    assert!(permitted_result.is_ok());
    assert!(allowed_prompt.lock().unwrap().is_some());
}
