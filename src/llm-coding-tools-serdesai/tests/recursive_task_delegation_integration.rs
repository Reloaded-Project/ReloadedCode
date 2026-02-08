//! Integration tests for recursive Task delegation (depth >= 2).
//!
//! Tests verify that nested subagent delegation chains work correctly
//! with allow/deny permission evaluation at each hop.

use async_trait::async_trait;
use indexmap::IndexMap;
use llm_coding_tools_agents::{AgentConfig, AgentMode, PermissionRule};
use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};
use llm_coding_tools_serdesai::{
    AgentRegistry, AgentRegistryEntry, RegistryAgent, RegistryAgentError, TaskDefinitionSnapshot,
    TaskRegistryHandle, TaskTargetSummary, TaskTool,
};
use serdes_ai::tools::{RunContext, Tool};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Converts a Ruleset back to PermissionRule config format.
///
/// This ensures test fixtures have realistic config.permission populated
/// from the same rules used for runtime evaluation.
fn permission_from_ruleset(ruleset: &Ruleset) -> IndexMap<String, PermissionRule> {
    let mut grouped: IndexMap<String, IndexMap<String, PermissionAction>> = IndexMap::new();
    for rule in ruleset.iter() {
        grouped
            .entry(rule.permission().to_string())
            .or_default()
            .insert(rule.pattern().to_string(), rule.action());
    }

    let mut permission = IndexMap::new();
    for (perm, patterns) in grouped {
        if patterns.len() == 1 {
            if let Some(action) = patterns.get("*") {
                permission.insert(perm, PermissionRule::Action(*action));
                continue;
            }
        }
        permission.insert(perm, PermissionRule::Pattern(patterns));
    }
    permission
}

/// Creates a TaskDefinitionSnapshot from a registry.
fn snapshot_from_registry<A>(registry: &AgentRegistry<A>) -> TaskDefinitionSnapshot {
    TaskDefinitionSnapshot {
        targets: registry
            .iter()
            .map(|(name, entry)| TaskTargetSummary {
                name: name.clone(),
                mode: entry.config.mode,
                tool_names: entry.tool_names.clone(),
            })
            .collect(),
    }
}

/// Mock agent that records prompts for verification.
struct ScriptedAgent {
    /// The response to return when prompted.
    response: String,
    /// Last prompt received (captured for verification).
    last_prompt: Arc<Mutex<Option<String>>>,
}

impl ScriptedAgent {
    fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            last_prompt: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl RegistryAgent<()> for ScriptedAgent {
    async fn prompt(&self, message: String, _deps: Arc<()>) -> Result<String, RegistryAgentError> {
        *self.last_prompt.lock().unwrap() = Some(message);
        Ok(self.response.clone())
    }
}

/// Creates a registry entry with the given name, mode, and permission rules.
fn make_entry(name: &str, mode: AgentMode, ruleset: Ruleset) -> AgentRegistryEntry<ScriptedAgent> {
    AgentRegistryEntry {
        config: AgentConfig {
            name: name.to_string(),
            mode,
            description: String::new(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: permission_from_ruleset(&ruleset),
            options: HashMap::new(),
            prompt: String::new(),
        },
        ruleset,
        tool_names: vec![],
        system_prompt: String::new(),
        agent: ScriptedAgent::new(format!("response-from-{}", name)),
    }
}

/// Creates a ruleset that allows specific targets.
fn rules_allow(targets: &[&str]) -> Ruleset {
    let mut ruleset = Ruleset::new();
    // Default deny
    ruleset.push(Rule::new("task", "*", PermissionAction::Deny));
    // Allow specific targets
    for target in targets {
        ruleset.push(Rule::new("task", *target, PermissionAction::Allow));
    }
    ruleset
}

/// Creates a ruleset that denies a specific target (with wildcard allow before it).
fn rules_deny(target: &str) -> Ruleset {
    let mut ruleset = Ruleset::new();
    // Allow all via wildcard
    ruleset.push(Rule::new("task", "*", PermissionAction::Allow));
    // Deny specific target
    ruleset.push(Rule::new("task", target, PermissionAction::Deny));
    ruleset
}

#[tokio::test]
async fn depth_2_allow_chain_succeeds() {
    // Agent A -> Agent B (A allows B, B has no allow entries - default-deny)
    let agent_a = make_entry("agent-a", AgentMode::Subagent, rules_allow(&["agent-b"]));
    let agent_b = make_entry("agent-b", AgentMode::Subagent, rules_allow(&[]));

    let registry = AgentRegistry::from_entries([
        ("agent-a".to_string(), agent_a),
        ("agent-b".to_string(), agent_b),
    ]);

    let handle = Arc::new(TaskRegistryHandle::from_registry(Arc::new(registry)));
    let snapshot = snapshot_from_registry(handle.get().unwrap());

    // Create TaskTool for agent-a (can delegate to agent-b)
    let task_tool = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-a",
        rules_allow(&["agent-b"]),
        snapshot,
        Arc::new(()),
    );

    let args = serde_json::json!({
        "description": "Delegate to B",
        "prompt": "Do something",
        "subagent_type": "agent-b"
    });

    let result = task_tool
        .call(&RunContext::minimal("test-model"), args)
        .await;

    assert!(
        result.is_ok(),
        "Expected successful delegation, got: {:?}",
        result
    );
}

#[tokio::test]
async fn depth_2_b_denies_but_a_to_b_succeeds() {
    // Agent A -> Agent B (A allows B, but B denies everyone)
    let agent_a = make_entry("agent-a", AgentMode::Subagent, rules_allow(&["agent-b"]));
    let agent_b = make_entry("agent-b", AgentMode::Subagent, rules_deny("*"));

    let registry = AgentRegistry::from_entries([
        ("agent-a".to_string(), agent_a),
        ("agent-b".to_string(), agent_b),
    ]);

    let handle = Arc::new(TaskRegistryHandle::from_registry(Arc::new(registry)));
    let snapshot = snapshot_from_registry(handle.get().unwrap());

    // Create TaskTool for agent-a (can delegate to agent-b)
    let task_tool = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-a",
        rules_allow(&["agent-b"]),
        snapshot,
        Arc::new(()),
    );

    let args = serde_json::json!({
        "description": "Delegate to B",
        "prompt": "Do something",
        "subagent_type": "agent-b"
    });

    let result = task_tool
        .call(&RunContext::minimal("test-model"), args)
        .await;

    // The call should succeed (agent-a can call agent-b), but if agent-b
    // were to try calling someone else, it would fail.
    assert!(result.is_ok(), "Expected successful call to allowed agent");
}

#[tokio::test]
async fn depth_2_fails_when_caller_denies_target() {
    // Agent A denies B, so A cannot delegate to B
    let agent_a = make_entry("agent-a", AgentMode::Subagent, rules_deny("agent-b"));
    let agent_b = make_entry("agent-b", AgentMode::Subagent, Ruleset::new());

    let registry = AgentRegistry::from_entries([
        ("agent-a".to_string(), agent_a),
        ("agent-b".to_string(), agent_b),
    ]);

    let handle = Arc::new(TaskRegistryHandle::from_registry(Arc::new(registry)));
    let snapshot = snapshot_from_registry(handle.get().unwrap());

    // Create TaskTool for agent-a (denies agent-b)
    let task_tool = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-a",
        rules_deny("agent-b"),
        snapshot,
        Arc::new(()),
    );

    let args = serde_json::json!({
        "description": "Delegate to B",
        "prompt": "Do something",
        "subagent_type": "agent-b"
    });

    let result = task_tool
        .call(&RunContext::minimal("test-model"), args)
        .await;

    assert!(
        result.is_err(),
        "Expected access denied when caller denies target"
    );
    let err = result.unwrap_err();
    let err_str = format!("{:?}", err);
    assert!(
        err_str.contains("Access denied") || err_str.contains("validation"),
        "Expected access denied error, got: {}",
        err_str
    );
}

#[tokio::test]
async fn depth_3_chain_with_runtime_permission_lookup() {
    // Agent A -> Agent B -> Agent C
    // A allows B, B allows C
    let agent_a = make_entry("agent-a", AgentMode::Subagent, rules_allow(&["agent-b"]));
    let agent_b = make_entry("agent-b", AgentMode::Subagent, rules_allow(&["agent-c"]));
    let agent_c = make_entry("agent-c", AgentMode::Subagent, rules_allow(&[]));

    let registry = AgentRegistry::from_entries([
        ("agent-a".to_string(), agent_a),
        ("agent-b".to_string(), agent_b),
        ("agent-c".to_string(), agent_c),
    ]);

    let handle = Arc::new(TaskRegistryHandle::from_registry(Arc::new(registry)));
    let snapshot = snapshot_from_registry(handle.get().unwrap());

    // Test: agent-a can call agent-b
    let task_a = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-a",
        rules_allow(&["agent-b"]),
        snapshot.clone(),
        Arc::new(()),
    );

    let result = task_a
        .call(
            &RunContext::minimal("test-model"),
            serde_json::json!({
                "description": "Delegate to B",
                "prompt": "Do something",
                "subagent_type": "agent-b"
            }),
        )
        .await;
    assert!(result.is_ok(), "agent-a should be able to call agent-b");

    // Test: agent-b can call agent-c
    let task_b = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-b",
        rules_allow(&["agent-c"]),
        snapshot.clone(),
        Arc::new(()),
    );

    let result = task_b
        .call(
            &RunContext::minimal("test-model"),
            serde_json::json!({
                "description": "Delegate to C",
                "prompt": "Do something else",
                "subagent_type": "agent-c"
            }),
        )
        .await;
    assert!(result.is_ok(), "agent-b should be able to call agent-c");

    // Test: agent-a cannot call agent-c directly (not in its allow list)
    let task_a_limited = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-a",
        rules_allow(&["agent-b"]), // Only allows agent-b, not agent-c
        snapshot,
        Arc::new(()),
    );

    let result = task_a_limited
        .call(
            &RunContext::minimal("test-model"),
            serde_json::json!({
                "description": "Try to delegate to C",
                "prompt": "Do something",
                "subagent_type": "agent-c"
            }),
        )
        .await;
    assert!(
        result.is_err(),
        "agent-a should not be able to call agent-c directly"
    );
}

#[test]
fn task_registry_handle_set_once_behavior() {
    let handle: TaskRegistryHandle<String> = TaskRegistryHandle::new();

    // First set should succeed
    let registry = Arc::new(AgentRegistry::from_entries([]));
    assert!(handle.set(Arc::clone(&registry)).is_ok());

    // Second set should fail
    let registry2 = Arc::new(AgentRegistry::from_entries([]));
    assert!(handle.set(registry2).is_err());
}

#[test]
fn task_definition_snapshot_default_is_empty() {
    let snapshot = TaskDefinitionSnapshot::default();
    assert!(snapshot.targets.is_empty());
}

#[tokio::test]
async fn task_tool_registry_caller_uses_per_agent_rules() {
    // Verify that runtime rules lookup uses per-agent ruleset from registry entry
    let mut agent_a_rules = Ruleset::new();
    agent_a_rules.push(Rule::new("task", "agent-b", PermissionAction::Allow));

    let mut agent_b_rules = Ruleset::new();
    agent_b_rules.push(Rule::new("task", "agent-c", PermissionAction::Allow));

    let agent_a = make_entry("agent-a", AgentMode::Subagent, agent_a_rules.clone());
    let agent_b = make_entry("agent-b", AgentMode::Subagent, agent_b_rules.clone());

    let registry = AgentRegistry::from_entries([
        ("agent-a".to_string(), agent_a),
        ("agent-b".to_string(), agent_b),
    ]);

    let handle = Arc::new(TaskRegistryHandle::from_registry(Arc::new(registry)));
    let snapshot = snapshot_from_registry(handle.get().unwrap());

    // Use empty build_rules - runtime lookup should use per-agent rules from registry
    let task_a = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "agent-a",
        Ruleset::new(), // Empty at build time
        snapshot,
        Arc::new(()),
    );

    // This should work because runtime lookup uses agent-a's rules from registry
    let result = task_a
        .call(
            &RunContext::minimal("test-model"),
            serde_json::json!({
                "description": "Delegate to B",
                "prompt": "Do something",
                "subagent_type": "agent-b"
            }),
        )
        .await;

    assert!(
        result.is_ok(),
        "Should use per-agent rules from registry: {:?}",
        result
    );
}

#[tokio::test]
async fn task_tool_registry_caller_missing_entry_defaults_to_deny() {
    // When caller name is not in registry, should default to deny-all
    let agent_b = make_entry("agent-b", AgentMode::Subagent, Ruleset::new());

    let registry = AgentRegistry::from_entries([("agent-b".to_string(), agent_b)]);

    let handle = Arc::new(TaskRegistryHandle::from_registry(Arc::new(registry)));
    let snapshot = snapshot_from_registry(handle.get().unwrap());

    // Use a caller name that doesn't exist in registry
    let task_unknown = TaskTool::for_registry_caller(
        Arc::clone(&handle),
        "unknown-caller",
        Ruleset::new(),
        snapshot,
        Arc::new(()),
    );

    let result = task_unknown
        .call(
            &RunContext::minimal("test-model"),
            serde_json::json!({
                "description": "Try to delegate",
                "prompt": "Do something",
                "subagent_type": "agent-b"
            }),
        )
        .await;

    // Should be denied because unknown-caller has no rules in registry
    assert!(result.is_err(), "Unknown caller should default to deny-all");
}

// Note: End-to-end tests with self-delegating agents would require a more complex
// setup to handle the circular dependency between agents and the registry.
// The existing tests above verify the core recursive delegation functionality:
// - depth_2_allow_chain_succeeds: Basic 1-hop delegation
// - depth_3_chain_with_runtime_permission_lookup: Multiple hops with separate TaskTool instances
// - task_tool_registry_caller_uses_per_agent_rules: Runtime permission lookup from registry
