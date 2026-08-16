//! Run event types: the framework-owned streaming item type.
//!
//! [`RunEvent`] is the item type a run stream yields. Adapters
//! translate their vendor-specific stream events into it, so consumers
//! match one stable framework-owned enum instead of vendor types.
//!
//! # Transcript distillation
//!
//! [`RunEvent::RunComplete`] carries a distilled transcript
//! ([`RunMessage`]): each message records its role, text, and tool
//! call/result summaries. The record serves display and audit;
//! consumers needing model-replay detail must use the underlying
//! agent directly.
//!
//! # Extensibility
//!
//! [`RunEvent`] is `#[non_exhaustive]`: variants may be appended
//! without a breaking release. Consumers match it with a wildcard arm.

use serde::{Deserialize, Serialize};

/// Framework-owned event yielded by a run stream.
///
/// One variant per observable streaming milestone: run start, text
/// and thinking deltas, tool activity (call start, call complete,
/// executed), output-ready, run complete, error, and cancellation.
///
/// The enum is `#[non_exhaustive]`: variants may be appended in a
/// future release without a breaking change, so matches outside this
/// crate need a wildcard arm.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunEvent {
    /// The run started.
    RunStart {
        /// Identifier of the started run.
        run_id: String,
    },
    /// Incremental assistant text arrived.
    TextDelta {
        /// Text fragment appended since the previous delta.
        text: String,
    },
    /// Incremental reasoning text arrived (reasoning models).
    ThinkingDelta {
        /// Thinking fragment appended since the previous delta.
        text: String,
    },
    /// A tool call started; its arguments may still be streaming.
    ToolCallStart {
        /// Name of the tool being called.
        tool_name: String,
        /// Call id correlating this call with its completion and result.
        tool_call_id: Option<String>,
    },
    /// A tool call's arguments finished streaming.
    ToolCallComplete {
        /// Name of the tool being called.
        tool_name: String,
        /// Call id correlating this call with its start and result.
        tool_call_id: Option<String>,
    },
    /// A tool finished executing.
    ToolExecuted {
        /// Name of the tool that ran.
        tool_name: String,
        /// Call id of the executed call.
        tool_call_id: Option<String>,
        /// Whether the tool reported success.
        success: bool,
        /// Error text when the tool failed.
        error: Option<String>,
    },
    /// The run's final output is ready to consume.
    OutputReady,
    /// The run completed.
    RunComplete {
        /// Identifier of the completed run.
        run_id: String,
        /// Distilled transcript of the run.
        messages: Vec<RunMessage>,
    },
    /// The run failed.
    Error {
        /// Human-readable description of the failure.
        message: String,
    },
    /// The run was cancelled.
    Cancelled {
        /// Partial text accumulated before cancellation.
        partial_text: Option<String>,
        /// Partial thinking content accumulated before cancellation.
        partial_thinking: Option<String>,
        /// Tool names whose calls were still in progress when
        /// cancelled.
        pending_tools: Vec<String>,
    },
}

/// Distilled transcript message carried by [`RunEvent::RunComplete`].
///
/// One participant turn: what it said ([`Self::text`]), which tools it
/// requested ([`Self::tool_calls`]), or which tool result it returns
/// ([`Self::tool_result`]). Missing fields mean the message carries no
/// content of that kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMessage {
    /// Author of the message.
    pub role: RunMessageRole,
    /// Text content of the message, if any.
    pub text: Option<String>,
    /// Tool calls the message requests, in order.
    pub tool_calls: Vec<RunToolCallSummary>,
    /// Tool result the message returns, if any.
    pub tool_result: Option<RunToolResultSummary>,
}

/// Author role of a [`RunMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunMessageRole {
    /// Framework-level instruction.
    System,
    /// Human input.
    User,
    /// Model output.
    Assistant,
    /// Tool output answering an assistant tool call.
    Tool,
}

/// Distilled summary of one tool call requested during a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunToolCallSummary {
    /// Name of the requested tool.
    pub tool_name: String,
    /// Call id correlating the call with its result.
    pub tool_call_id: Option<String>,
    /// JSON-serialized arguments, when the call carries any.
    pub arguments_json: Option<String>,
}

/// Distilled summary of one tool result returned during a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunToolResultSummary {
    /// Call id of the tool call this result answers.
    pub tool_call_id: Option<String>,
    /// Result payload rendered as text for display and audit.
    pub output: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full-shape transcript: every role, a tool call, and a tool result.
    fn complete_transcript() -> Vec<RunMessage> {
        vec![
            RunMessage {
                role: RunMessageRole::System,
                text: Some("sys".into()),
                tool_calls: Vec::new(),
                tool_result: None,
            },
            RunMessage {
                role: RunMessageRole::User,
                text: Some("read a.txt".into()),
                tool_calls: Vec::new(),
                tool_result: None,
            },
            RunMessage {
                role: RunMessageRole::Assistant,
                text: Some("checking".into()),
                tool_calls: vec![RunToolCallSummary {
                    tool_name: "read_file".into(),
                    tool_call_id: Some("call_1".into()),
                    arguments_json: Some(r#"{"path":"a.txt"}"#.into()),
                }],
                tool_result: None,
            },
            RunMessage {
                role: RunMessageRole::Tool,
                text: None,
                tool_calls: Vec::new(),
                tool_result: Some(RunToolResultSummary {
                    tool_call_id: Some("call_1".into()),
                    output: "contents".into(),
                }),
            },
        ]
    }

    #[test]
    fn run_complete_serde_roundtrip_preserves_transcript() {
        let event = RunEvent::RunComplete {
            run_id: "run-42".into(),
            messages: complete_transcript(),
        };
        let json = serde_json::to_string(&event).unwrap();
        // Pin the wire shape: variants are externally tagged, so the
        // variant name is the JSON object key consumers see.
        assert!(json.starts_with("{\"RunComplete\":"));
        let restored: RunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }

    #[test]
    fn run_event_variants_serde_roundtrip() {
        let events = vec![
            RunEvent::RunStart {
                run_id: "run-42".into(),
            },
            RunEvent::TextDelta {
                text: "chunk".into(),
            },
            RunEvent::ThinkingDelta {
                text: "thought".into(),
            },
            RunEvent::ToolCallStart {
                tool_name: "read_file".into(),
                tool_call_id: Some("call_1".into()),
            },
            RunEvent::ToolCallComplete {
                tool_name: "read_file".into(),
                tool_call_id: Some("call_1".into()),
            },
            RunEvent::ToolExecuted {
                tool_name: "read_file".into(),
                tool_call_id: Some("call_1".into()),
                success: false,
                error: Some("missing".into()),
            },
            RunEvent::OutputReady,
            RunEvent::Error {
                message: "boom".into(),
            },
            RunEvent::Cancelled {
                partial_text: Some("partial".into()),
                partial_thinking: None,
                pending_tools: vec!["read_file".into()],
            },
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let restored: RunEvent = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, event);
        }
    }
}
