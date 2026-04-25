//! Task tool input/output types for registry-driven Task implementations.
//!
//! Provides [`TaskInput`] and [`TaskOutput`] types used by framework-specific
//! Task tools (e.g., serdesAI). These types are DTOs for task execution
//! and do not include a core runner abstraction.
//!
//! Framework-specific Task tools use registry-driven AgentCatalog for agent lookup.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Shared runtime settings for Task delegation.
///
/// # Delegation depth
///
/// `current_depth` starts at `0` for the root agent and increments by `1` for
/// each Task hop. With the default [`TaskSettings::DEFAULT_MAX_DEPTH`] of `3`, three
/// delegated hops are allowed before Task must stop delegating further.
///
/// | `current_depth` | Allowed? |
/// |-----------------|----------|
/// | `0`             | yes      |
/// | `1`             | yes      |
/// | `2`             | yes      |
/// | `3`             | no       |
///
/// This prevents unbounded recursion (e.g. `A -> A -> A -> …`) without
/// rejecting legitimate self-delegation or diamond-shaped call graphs
/// (e.g. `A -> B -> A`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSettings {
    max_depth: u8,
}

impl Default for TaskSettings {
    #[inline]
    fn default() -> Self {
        Self {
            max_depth: Self::DEFAULT_MAX_DEPTH,
        }
    }
}

impl TaskSettings {
    /// Default maximum number of Task delegation hops.
    pub const DEFAULT_MAX_DEPTH: u8 = 3;

    /// Creates settings with a custom maximum delegation depth.
    ///
    /// A value of `0` disables further Task delegation.
    #[inline]
    pub const fn with_max_depth(max_depth: u8) -> Self {
        Self { max_depth }
    }

    /// Returns the maximum number of Task delegation hops.
    #[inline]
    pub const fn max_depth(self) -> u8 {
        self.max_depth
    }

    /// Returns whether another Task hop is allowed at `current_depth`.
    #[inline]
    pub const fn allows_delegation(self, current_depth: u8) -> bool {
        current_depth < self.max_depth
    }
}

/// Input for task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    /// Short description (3-5 words) of the task.
    pub description: String,
    /// The prompt/task for the agent to perform.
    pub prompt: String,
    /// The subagent type/name to invoke.
    pub subagent_type: String,
    /// Optional command that triggered this task (for context).
    pub command: Option<String>,
}

/// Output from task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    /// The text summary/response from the agent.
    pub summary: String,
    /// Optional metadata from the execution.
    pub metadata: Option<Value>,
}

impl TaskOutput {
    /// Creates a new task output with just a summary.
    #[inline]
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            metadata: None,
        }
    }

    /// Sets metadata.
    #[inline]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::TaskSettings;

    #[test]
    fn task_settings_allow_delegation_only_below_max_depth() {
        let settings = TaskSettings::with_max_depth(3);

        assert!(settings.allows_delegation(0));
        assert!(settings.allows_delegation(2));
        assert!(!settings.allows_delegation(3));
    }
}
