//! Task tool for invoking subagents (serdesAI adapter).
//!
//! Thin tool that validates access and dispatches to prebuilt agents in a registry.

use crate::convert::to_serdes_result;
use crate::registry::{AgentRegistry, RegistryAgent};
use async_trait::async_trait;
use llm_coding_tools_agents::AgentMode;
use llm_coding_tools_core::permissions::{PermissionAction, Ruleset};
use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::tool_names;
use llm_coding_tools_core::tools::TaskInput;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, SchemaBuilder, Tool, ToolDefinition, ToolError, ToolResult};
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

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

/// Summary of a Task target agent for definition-time snapshot capture.
#[derive(Debug, Clone)]
pub struct TaskTargetSummary {
    /// Agent name.
    pub name: String,
    /// Agent mode (controls invocability).
    pub mode: AgentMode,
    /// Tool names available to the agent.
    pub tool_names: Vec<String>,
}

impl TaskTargetSummary {
    #[inline]
    fn is_invocable(&self) -> bool {
        matches!(self.mode, AgentMode::Subagent | AgentMode::All)
    }
}

/// Snapshot of Task definition metadata captured at build time.
///
/// This avoids runtime registry dependency during tool definition generation.
#[derive(Debug, Clone, Default)]
pub struct TaskDefinitionSnapshot {
    /// Available Task targets at build time.
    pub targets: Vec<TaskTargetSummary>,
}

/// Handle for lazy registry initialization in recursive Task wiring.
///
/// Enables two-phase construction where Task tools are created before
/// the registry is fully assembled, then wired together after.
pub struct TaskRegistryHandle<A> {
    registry: OnceLock<Arc<AgentRegistry<A>>>,
}

impl<A> TaskRegistryHandle<A> {
    /// Creates a new uninitialized handle.
    pub fn new() -> Self {
        Self {
            registry: OnceLock::new(),
        }
    }

    /// Creates a handle pre-initialized with a registry.
    pub fn from_registry(registry: Arc<AgentRegistry<A>>) -> Self {
        let handle = Self::new();
        let _ = handle.registry.set(registry);
        handle
    }

    /// Sets the registry. Returns Err if already set.
    pub fn set(&self, registry: Arc<AgentRegistry<A>>) -> Result<(), Arc<AgentRegistry<A>>> {
        self.registry.set(registry)
    }

    /// Returns the registry if initialized.
    pub fn get(&self) -> Option<&Arc<AgentRegistry<A>>> {
        self.registry.get()
    }
}

impl<A> Default for TaskRegistryHandle<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Authority source for Task permission evaluation.
enum TaskCallerAuthority {
    /// Static ruleset from legacy construction.
    StaticRules(Ruleset),
    /// Registry-caller with name for runtime lookup and build-time rules fallback.
    RegistryCaller {
        caller_name: String,
        build_rules: Ruleset,
    },
}

/// Task tool for serdesAI framework.
///
/// Validates access, builds the request message, and dispatches to stored agents.
pub struct TaskTool<A, Deps>
where
    A: RegistryAgent<Deps>,
{
    registry: Arc<TaskRegistryHandle<A>>,
    authority: TaskCallerAuthority,
    definition_snapshot: TaskDefinitionSnapshot,
    deps: Arc<Deps>,
}

impl<A, Deps> TaskTool<A, Deps>
where
    A: RegistryAgent<Deps>,
{
    /// Creates a new Task tool with the given registry, caller permissions, and deps.
    ///
    /// Parameters:
    /// - `registry`: serdesAI agent registry.
    /// - `caller_rules`: permission rules for the calling agent.
    /// - `deps`: dependencies passed to registry agents.
    ///
    /// Returns: a new [`TaskTool`].
    pub fn new(registry: Arc<AgentRegistry<A>>, caller_rules: Ruleset, deps: Arc<Deps>) -> Self {
        let definition_snapshot = TaskDefinitionSnapshot {
            targets: registry
                .iter()
                .map(|(name, entry)| TaskTargetSummary {
                    name: name.clone(),
                    mode: entry.config.mode,
                    tool_names: entry.tool_names.clone(),
                })
                .collect(),
        };
        Self {
            registry: Arc::new(TaskRegistryHandle::from_registry(registry)),
            authority: TaskCallerAuthority::StaticRules(caller_rules),
            definition_snapshot,
            deps,
        }
    }

    /// Creates a Task tool for a registry-caller with snapshot-based definition.
    ///
    /// Parameters:
    /// - `registry`: registry handle for runtime lookup.
    /// - `caller_name`: name of the calling agent for runtime rules lookup.
    /// - `caller_rules`: build-time ruleset for definition generation.
    /// - `definition_snapshot`: precomputed target metadata.
    /// - `deps`: dependencies passed to registry agents.
    ///
    /// Returns: a new [`TaskTool`] configured for recursive delegation.
    pub fn for_registry_caller(
        registry: Arc<TaskRegistryHandle<A>>,
        caller_name: impl Into<String>,
        caller_rules: Ruleset,
        definition_snapshot: TaskDefinitionSnapshot,
        deps: Arc<Deps>,
    ) -> Self {
        Self {
            registry,
            authority: TaskCallerAuthority::RegistryCaller {
                caller_name: caller_name.into(),
                build_rules: caller_rules,
            },
            definition_snapshot,
            deps,
        }
    }

    fn definition_rules(&self) -> &Ruleset {
        match &self.authority {
            TaskCallerAuthority::StaticRules(ruleset) => ruleset,
            TaskCallerAuthority::RegistryCaller { build_rules, .. } => build_rules,
        }
    }

    fn resolve_registry(&self) -> Result<&AgentRegistry<A>, ToolError> {
        self.registry
            .get()
            .map(|registry| registry.as_ref())
            .ok_or_else(|| {
                ToolError::execution_failed("Task registry is not initialized".to_string())
            })
    }

    fn resolve_runtime_rules<'a>(&'a self, registry: &'a AgentRegistry<A>) -> Cow<'a, Ruleset> {
        match &self.authority {
            TaskCallerAuthority::StaticRules(ruleset) => Cow::Borrowed(ruleset),
            TaskCallerAuthority::RegistryCaller { caller_name, .. } => registry
                .get(caller_name)
                .map(|entry| Cow::Borrowed(&entry.ruleset))
                .unwrap_or_else(|| Cow::Owned(Ruleset::new())),
        }
    }
}

#[async_trait]
impl<A, Deps, RuntimeDeps> Tool<RuntimeDeps> for TaskTool<A, Deps>
where
    A: RegistryAgent<Deps> + 'static,
    Deps: Send + Sync + 'static,
    RuntimeDeps: Send + Sync,
{
    fn definition(&self) -> ToolDefinition {
        // Build the Task tool description from invocable + permitted agents
        let mut names: Vec<_> = self
            .definition_snapshot
            .targets
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        names.sort_unstable();

        let mut lines = Vec::with_capacity(names.len());
        for name in names {
            let target = match self
                .definition_snapshot
                .targets
                .iter()
                .find(|target| target.name == name)
            {
                Some(target) => target,
                None => continue,
            };
            if !target.is_invocable() {
                continue;
            }
            if self.definition_rules().evaluate(tool_names::TASK, name) != PermissionAction::Allow {
                continue;
            }
            lines.push(format!("- {}: {}", name, target.tool_names.join(", ")));
        }

        let description = if lines.is_empty() {
            "Task tool is not available - no accessible agents.".to_string()
        } else {
            const TEMPLATE: &str = r#"Launch a new agent to handle complex, multistep tasks autonomously.

Available agent types and the tools they have access to:
{agents}

When using the Task tool, you must specify a subagent_type parameter to select which agent type to use."#;
            TEMPLATE.replace("{agents}", &lines.join("\n"))
        };

        ToolDefinition::new(tool_names::TASK, description).with_parameters(
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

    async fn call(&self, _ctx: &RunContext<RuntimeDeps>, args: serde_json::Value) -> ToolResult {
        let args: TaskArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error(tool_names::TASK, None, e.to_string()))?;

        let input: TaskInput = args.into();
        let registry = self.resolve_registry()?;
        let entry = match registry.get(&input.subagent_type) {
            Some(entry) => entry,
            None => {
                return Err(ToolError::validation_error(
                    tool_names::TASK,
                    Some("subagent_type".to_string()),
                    format!("Unknown agent type: {}", input.subagent_type),
                ));
            }
        };

        if !entry.is_invocable() {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!(
                    "Agent '{}' is not available for task invocation",
                    input.subagent_type
                ),
            ));
        }

        let caller_rules = self.resolve_runtime_rules(registry);
        if caller_rules.evaluate(tool_names::TASK, &input.subagent_type) != PermissionAction::Allow
        {
            return Err(ToolError::validation_error(
                tool_names::TASK,
                Some("subagent_type".to_string()),
                format!(
                    "Access denied: cannot invoke agent '{}'",
                    input.subagent_type
                ),
            ));
        }

        // Build the required task context user message
        let mut message = String::with_capacity(input.prompt.len() + 128);
        message.push_str("<task_context>\n");
        message.push_str("description: ");
        message.push_str(&input.description);
        message.push('\n');
        message.push_str("command: ");
        if let Some(command) = &input.command {
            message.push_str(command);
        }
        message.push('\n');
        message.push_str("session_id: ");
        if let Some(session_id) = &input.session_id {
            message.push_str(session_id);
        }
        message.push('\n');
        message.push_str("</task_context>\n\n<task_prompt>\n");
        message.push_str(&input.prompt);
        message.push_str("</task_prompt>");

        let result = entry
            .agent
            .prompt(message, Arc::clone(&self.deps))
            .await
            .map_err(|err| {
                ToolError::execution_failed(format!("Task execution failed: {}", err))
            })?;

        to_serdes_result(
            tool_names::TASK,
            Ok(llm_coding_tools_core::ToolOutput::new(result)),
        )
    }
}

impl<A: RegistryAgent<Deps>, Deps> ToolContext for TaskTool<A, Deps> {
    const NAME: &'static str = tool_names::TASK;

    fn context(&self) -> &'static str {
        "Use the Task tool to delegate complex, multi-step tasks to specialized subagents."
    }
}

#[cfg(test)]
mod tests;
