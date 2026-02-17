//! Task tool input/output types for registry-driven Task implementations.
//!
//! Provides [`TaskInput`] and [`TaskOutput`] types used by framework-specific
//! Task tools (e.g., serdesAI). These types are DTOs for task execution
//! and do not include a core runner abstraction.
//!
//! Framework-specific Task tools use registry-driven AgentCatalog for agent lookup.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Input for task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInput {
    /// Short description (3-5 words) of the task.
    pub description: String,
    /// The prompt/task for the agent to perform.
    pub prompt: String,
    /// The subagent type/name to invoke.
    pub subagent_type: String,
    /// Optional session ID to continue an existing task session.
    pub session_id: Option<String>,
    /// Optional command that triggered this task (for context).
    pub command: Option<String>,
}

/// Output from task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    /// The text summary/response from the agent.
    pub summary: String,
    /// Session ID for continuation (if supported by implementation).
    pub session_id: Option<String>,
    /// Optional metadata from the execution.
    pub metadata: Option<Value>,
}

impl TaskOutput {
    /// Creates a new task output with just a summary.
    #[inline]
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            session_id: None,
            metadata: None,
        }
    }

    /// Sets the session ID.
    #[inline]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Sets metadata.
    #[inline]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}
