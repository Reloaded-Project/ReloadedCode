//! Run event types: the framework-owned streaming item type, plus the
//! hook that intercepts each event before publication.
//!
//! [`RunEvent`] is the item type a run stream yields. Adapters
//! translate their vendor-specific stream events into it, so consumers
//! match one stable framework-owned enum instead of vendor types.
//!
//! # Run-event hooks
//!
//! [`RunEventHook`] sees each streamed event before publication:
//! observe, rewrite, or suppress. It fires only on the streaming
//! path; the run boundary hook [`RunHook`] fires only on `run()`.
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
//!
//! [`RunHook`]: crate::hooks::RunHook

use crate::ToolError;
use serde::{Deserialize, Serialize};

/// Static context for a run-event hook call.
///
/// Names only. The run id is deliberately absent: it is learnable
/// from the [`RunEvent::RunStart`] and [`RunEvent::RunComplete`]
/// events the stream itself yields.
#[derive(Debug)]
pub struct RunEventContext<'a> {
    /// Name of the agent whose stream produced the event.
    pub agent_name: &'a str,
    /// Name of the model generating the event stream.
    pub model_name: &'a str,
}

/// Publish decision for one event after the run-event hook chain.
///
/// `Ok(Some(event))` publishes the event, possibly rewritten by a
/// hook; `Ok(None)` suppresses it; `Err` carries the first hook error.
pub type RunEventHookResult = Result<Option<RunEvent>, ToolError>;

/// Framework-owned event yielded by a run stream.
///
/// One variant per observable streaming milestone: run start, step
/// boundaries, context telemetry, text and thinking deltas, tool
/// activity (call start, argument deltas, call complete, executed),
/// output-ready, run complete, error, and cancellation.
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
    /// A model-request step started; the step's tool events may follow
    /// the matching [`RunEvent::StepEnd`] (see that variant).
    ///
    /// Optional: emitted only by backends that report step boundaries;
    /// absence is normal.
    StepStart {
        /// Index of the started step; numbering is backend-defined.
        step: u32,
    },
    /// Context-size telemetry measured while building a model request.
    ///
    /// Optional: emitted only by backends that report context metrics;
    /// absence is normal.
    ContextInfo {
        /// Estimated token count of the request.
        estimated_tokens: usize,
        /// Serialized request size in bytes (messages plus tools).
        request_bytes: usize,
        /// Model's context window limit, when known.
        context_limit: Option<u64>,
    },
    /// Context was compressed to fit within limits.
    ///
    /// Optional: emitted only by backends that compress context and
    /// report it; absence is normal.
    ContextCompressed {
        /// Token count before compression.
        original_tokens: usize,
        /// Token count after compression.
        compressed_tokens: usize,
        /// Strategy used, e.g. "truncate" or "summarize".
        strategy: String,
        /// Number of messages before compression.
        messages_before: usize,
        /// Number of messages after compression.
        messages_after: usize,
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
    /// Incremental tool-call argument fragment arrived.
    ///
    /// Optional: a backend may omit it; absence is normal. Complete
    /// arguments remain available in the [`RunEvent::RunComplete`]
    /// transcript as [`RunToolCallSummary::arguments_json`].
    ToolCallDelta {
        /// Call id correlating this fragment with its call.
        tool_call_id: Option<String>,
        /// Argument fragment appended since the previous delta.
        delta: String,
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
    /// A model-request step's response finished.
    ///
    /// Backends that execute tools inside a step emit this before the
    /// step's tool calls run, so tool events may arrive after the
    /// matching [`RunEvent::StepEnd`].
    ///
    /// Optional: emitted only by backends that report step boundaries;
    /// absence is normal.
    StepEnd {
        /// Index of the finished step, matching its
        /// [`RunEvent::StepStart`].
        step: u32,
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

/// Mode-scoped hook for streamed run events.
///
/// Fires only on the streaming path: each event a run stream yields
/// passes every registered run-event hook, in registration order,
/// before the stream consumer sees it.
///
/// Per event, a hook may:
/// - observe: return the event unchanged,
/// - rewrite: return a changed event,
/// - suppress: return `Ok(None)`.
///
/// Hooks run synchronously at token rate, so per-event work stays
/// cheap (plain string transforms). Each call sees exactly one event;
/// a hook needing cross-event context buffers it internally.
///
/// [`RunHook`]: crate::hooks::RunHook
pub trait RunEventHook: Send + Sync + 'static {
    /// Observes, rewrites, or suppresses one streamed event.
    ///
    /// # Errors
    /// Returns `ToolError` when the hook fails; dispatch stops at the
    /// first error and returns it to the caller.
    fn hook(&self, ctx: &RunEventContext<'_>, event: RunEvent) -> RunEventHookResult;
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
            RunEvent::StepStart { step: 0 },
            RunEvent::ContextInfo {
                estimated_tokens: 128,
                request_bytes: 512,
                context_limit: Some(8192),
            },
            RunEvent::ContextInfo {
                estimated_tokens: 128,
                request_bytes: 512,
                context_limit: None,
            },
            RunEvent::ContextCompressed {
                original_tokens: 9000,
                compressed_tokens: 4000,
                strategy: "truncate".into(),
                messages_before: 20,
                messages_after: 6,
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
            RunEvent::ToolCallDelta {
                tool_call_id: Some("call_1".into()),
                delta: "{\"path\":".into(),
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
            RunEvent::StepEnd { step: 0 },
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
