//! Vendor stream events mapped to framework-owned run events.
//!
//! [`RunEventStream`] wraps the vendor [`AgentStream`] and lazily maps each
//! [`AgentStreamEvent`] to a [`RunEvent`] as the consumer polls it. SerdesAI
//! event types stay inside this module; consumers of
//! [`HookedAgent::run_stream`][task] only ever see [`RunEvent`] items.
//!
//! # Optional events
//!
//! Step boundaries, context telemetry, and streamed tool-call
//! arguments map through when the vendor reports them. The vendor
//! delivers whole tool-call arguments as a single delta when the
//! model does not stream fragments, so [`RunEvent::ToolCallDelta`]
//! items arrive for every tool call with non-empty arguments.
//!
//! Ordering caveat: the vendor emits `ResponseComplete` before
//! executing a step's tool calls, so [`RunEvent::StepEnd`] closes the
//! step before its [`RunEvent::ToolExecuted`] items arrive.
//!
//! The match over [`AgentStreamEvent`] is exhaustive: a new vendor
//! variant fails compilation here, keeping vendor coupling inside
//! this module.
//!
//! [task]: super::task::HookedAgent::run_stream

use futures::{Stream, StreamExt};
use reloaded_code_core::hooks::{
    RunEvent, RunMessage, RunMessageRole, RunToolCallSummary, RunToolResultSummary,
};
use serdes_ai::core::messages::{RetryContent, ToolCallArgs, ToolReturnContent};
use serdes_ai::core::{
    ModelRequest, ModelRequestPart, ModelResponse, ModelResponsePart, UserContent,
};
use serdes_ai::{AgentStream, AgentStreamEvent};
use std::pin::Pin;
use std::task::{Context, Poll};

/// Lazy caller-driven [`Stream`] mapping a vendor [`AgentStream`] to
/// framework-owned [`RunEvent`] items.
///
/// Polling this stream drives the vendor stream, which the vendor already
/// runs on its own background task; no channel, spawn, or shared agent
/// handle is added on this side.
pub(super) struct RunEventStream {
    /// Owned vendor stream, driven by the vendor's own background task.
    inner: AgentStream,
}

impl RunEventStream {
    /// Wraps an already-started vendor stream.
    pub(super) fn new(inner: AgentStream) -> Self {
        Self { inner }
    }
}

impl Stream for RunEventStream {
    type Item = Result<RunEvent, serdes_ai::agent::AgentRunError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let inner = &mut self.get_mut().inner;
        match inner.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok(event))) => Poll::Ready(Some(Ok(map_vendor_event(event)))),
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Maps one vendor event to its framework-owned counterpart.
///
/// The match is exhaustive over the vendor enum, so vendor drift
/// fails compilation here instead of leaking vendor types.
fn map_vendor_event(event: AgentStreamEvent) -> RunEvent {
    match event {
        AgentStreamEvent::RunStart { run_id } => RunEvent::RunStart { run_id },
        AgentStreamEvent::RequestStart { step } => RunEvent::StepStart { step },
        AgentStreamEvent::ContextInfo {
            estimated_tokens,
            request_bytes,
            context_limit,
        } => RunEvent::ContextInfo {
            estimated_tokens,
            request_bytes,
            context_limit,
        },
        AgentStreamEvent::ContextCompressed {
            original_tokens,
            compressed_tokens,
            strategy,
            messages_before,
            messages_after,
        } => RunEvent::ContextCompressed {
            original_tokens,
            compressed_tokens,
            strategy,
            messages_before,
            messages_after,
        },
        AgentStreamEvent::TextDelta { text } => RunEvent::TextDelta { text },
        AgentStreamEvent::ThinkingDelta { text } => RunEvent::ThinkingDelta { text },
        AgentStreamEvent::ToolCallStart {
            tool_name,
            tool_call_id,
        } => RunEvent::ToolCallStart {
            tool_name,
            tool_call_id,
        },
        AgentStreamEvent::ToolCallDelta {
            delta,
            tool_call_id,
        } => RunEvent::ToolCallDelta {
            tool_call_id,
            delta,
        },
        AgentStreamEvent::ToolCallComplete {
            tool_name,
            tool_call_id,
        } => RunEvent::ToolCallComplete {
            tool_name,
            tool_call_id,
        },
        AgentStreamEvent::ToolExecuted {
            tool_name,
            tool_call_id,
            success,
            error,
        } => RunEvent::ToolExecuted {
            tool_name,
            tool_call_id,
            success,
            error,
        },
        AgentStreamEvent::ResponseComplete { step } => RunEvent::StepEnd { step },
        AgentStreamEvent::OutputReady => RunEvent::OutputReady,
        AgentStreamEvent::RunComplete { run_id, messages } => RunEvent::RunComplete {
            run_id,
            messages: distill_messages(messages),
        },
        AgentStreamEvent::Error { message } => RunEvent::Error { message },
        AgentStreamEvent::Cancelled {
            partial_text,
            partial_thinking,
            pending_tools,
        } => RunEvent::Cancelled {
            partial_text,
            partial_thinking,
            pending_tools,
        },
    }
}

/// Distills the vendor run transcript into framework-owned records.
///
/// One [`RunMessage`] per vendor part: system prompts, user prompts, and
/// retry feedback map to their authoring roles; tool returns map to
/// [`RunMessageRole::Tool`] with a [`RunToolResultSummary`]; model
/// responses map to [`RunMessageRole::Assistant`] with joined text plus
/// [`RunToolCallSummary`] entries. Thinking and file parts carry no
/// distilled representation and are skipped.
fn distill_messages(messages: Vec<ModelRequest>) -> Vec<RunMessage> {
    // Upper bound: one distilled message per part across all requests.
    let part_count = messages.iter().map(|message| message.parts.len()).sum();
    let mut distilled = Vec::with_capacity(part_count);
    for message in messages {
        for part in message.parts {
            match part {
                ModelRequestPart::SystemPrompt(part) => {
                    distilled.push(authored_message(RunMessageRole::System, part.content));
                }
                ModelRequestPart::UserPrompt(part) => {
                    distilled.push(authored_message(
                        RunMessageRole::User,
                        user_content_text(part.content),
                    ));
                }
                ModelRequestPart::RetryPrompt(part) => {
                    // Plain-text retry content moves its owned String;
                    // structured content still renders through the vendor
                    // accessor, which only borrows.
                    let text = match part.content {
                        RetryContent::Text(text) => text,
                        other => other.message().to_owned(),
                    };
                    distilled.push(authored_message(RunMessageRole::User, text));
                }
                ModelRequestPart::ToolReturn(part) => {
                    // Text returns (the common large case) move their
                    // owned String; other variants render through the
                    // vendor accessor, which clones.
                    let output = match part.content {
                        ToolReturnContent::Text { content } => content,
                        other => other.to_string_content(),
                    };
                    distilled.push(tool_result_message(part.tool_call_id, output));
                }
                ModelRequestPart::BuiltinToolReturn(part) => {
                    // Structured content (search results, code output) has
                    // no text projection; serialize it so the audit trail
                    // keeps it.
                    let output = serde_json::to_string(&part.content)
                        .unwrap_or_else(|_| format!("{:?}", part.content));
                    distilled.push(tool_result_message(Some(part.tool_call_id), output));
                }
                ModelRequestPart::ModelResponse(response) => {
                    if let Some(message) = distill_model_response(*response) {
                        distilled.push(message);
                    }
                }
            }
        }
    }
    distilled
}

/// Builds a text-only [`RunMessage`] for an authoring-side part.
fn authored_message(role: RunMessageRole, text: String) -> RunMessage {
    RunMessage {
        role,
        text: Some(text),
        tool_calls: Vec::new(),
        tool_result: None,
    }
}

/// Distills one assistant model response.
///
/// Returns `None` when the response carries no distilled content
/// (thinking-only, file-only, or empty).
fn distill_model_response(response: ModelResponse) -> Option<RunMessage> {
    // One sizing scan gives both outputs exact capacity, plus the text
    // part count: a single text part (the common shape) moves its
    // String instead of copying through a joined buffer.
    let mut text_len = 0usize;
    let mut text_part_count = 0usize;
    let mut tool_call_count = 0usize;
    for part in &response.parts {
        match part {
            ModelResponsePart::Text(text) => {
                text_len += text.content.len();
                text_part_count += 1;
            }
            ModelResponsePart::ToolCall(_) => tool_call_count += 1,
            _ => {}
        }
    }
    let mut tool_calls = Vec::with_capacity(tool_call_count);
    let mut joined = (text_part_count != 1).then(|| String::with_capacity(text_len));
    let mut single: Option<String> = None;
    // Consuming the parts moves the call fields instead of cloning.
    for part in response.parts {
        match part {
            ModelResponsePart::Text(part_text) => {
                if let Some(buffer) = joined.as_mut() {
                    buffer.push_str(&part_text.content);
                } else {
                    single = Some(part_text.content);
                }
            }
            ModelResponsePart::ToolCall(call) => tool_calls.push(RunToolCallSummary {
                tool_name: call.tool_name,
                tool_call_id: call.tool_call_id,
                // Raw-string args move their owned String; parsed JSON
                // serializes through the vendor accessor.
                arguments_json: match call.args {
                    ToolCallArgs::String(raw) => Some(raw),
                    other => other.to_json_string().ok(),
                },
            }),
            _ => {}
        }
    }
    // Multi-text responses keep their joined buffer; both paths drop
    // to `None` when no text survived.
    let text = single
        .or_else(|| joined.filter(|joined| !joined.is_empty()))
        .filter(|text| !text.is_empty());
    if text.is_none() && tool_calls.is_empty() {
        return None;
    }
    Some(RunMessage {
        role: RunMessageRole::Assistant,
        text,
        tool_calls,
        tool_result: None,
    })
}

/// Builds a [`RunMessageRole::Tool`] record answering a tool call.
fn tool_result_message(tool_call_id: Option<String>, output: String) -> RunMessage {
    RunMessage {
        role: RunMessageRole::Tool,
        text: None,
        tool_calls: Vec::new(),
        tool_result: Some(RunToolResultSummary {
            tool_call_id,
            output,
        }),
    }
}

/// Renders user prompt content as audit text.
///
/// Plain text passes through; multi-part prompts are serialized so image
/// and mixed content stay observable in the transcript.
fn user_content_text(content: UserContent) -> String {
    match content {
        UserContent::Text(text) => text,
        UserContent::Parts(parts) => {
            serde_json::to_string(&parts).unwrap_or_else(|_| format!("{parts:?}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::AgentBuildContext;
    use crate::agent_runtime::task::HookedAgent;
    use crate::agent_runtime::test_stubs::{
        SerdesTestFactory, agent, allow_tools, catalog, credentials, workspace_root,
    };
    use crate::mock::{FunctionModel, Streamed, tool_then_text};
    use futures::StreamExt;
    use reloaded_code_agents::{AgentCatalog, AgentDefaults, AgentMode, AgentRuntimeBuilder};
    use reloaded_code_core::hooks::{
        HookRunContext, HookSet, RunConfig, RunHook, RunHookFuture, RunOriginal,
    };
    use reloaded_code_core::{ToolCatalogEntry, ToolCatalogKind};
    use rstest::rstest;
    use serde_json::json;
    use serdes_ai::core::messages::request::RetryPromptPart;
    use serdes_ai::core::{
        BuiltinToolReturnContent, BuiltinToolReturnPart, SystemPromptPart, ToolReturnPart,
        UserPromptPart,
    };
    use serdes_ai_models::{ModelError, ModelProfile};
    use std::sync::{Arc, Mutex};

    // ========================================================================
    // Vendor event mapping
    // ========================================================================

    /// Optional events map field-for-field when the backend emits them;
    /// absence is normal on other backends. Whole-event equality pins
    /// variant identity too, so a mislabelled arm fails even when the
    /// field shapes compile.
    #[rstest]
    #[case::step_start(
        AgentStreamEvent::RequestStart { step: 2 },
        RunEvent::StepStart { step: 2 }
    )]
    #[case::step_end(
        AgentStreamEvent::ResponseComplete { step: 2 },
        RunEvent::StepEnd { step: 2 }
    )]
    #[case::context_info(
        AgentStreamEvent::ContextInfo {
            estimated_tokens: 128,
            request_bytes: 512,
            context_limit: Some(8192),
        },
        RunEvent::ContextInfo {
            estimated_tokens: 128,
            request_bytes: 512,
            context_limit: Some(8192),
        }
    )]
    #[case::context_compressed(
        AgentStreamEvent::ContextCompressed {
            original_tokens: 10,
            compressed_tokens: 5,
            strategy: "truncate".into(),
            messages_before: 2,
            messages_after: 1,
        },
        RunEvent::ContextCompressed {
            original_tokens: 10,
            compressed_tokens: 5,
            strategy: "truncate".into(),
            messages_before: 2,
            messages_after: 1,
        }
    )]
    // The vendor delivers whole arguments as one delta when the model
    // does not stream fragments; fragment shape pinned for pass-through.
    #[case::tool_call_delta(
        AgentStreamEvent::ToolCallDelta {
            delta: "{\"a\":".into(),
            tool_call_id: Some("call_1".into()),
        },
        RunEvent::ToolCallDelta {
            tool_call_id: Some("call_1".into()),
            delta: "{\"a\":".into(),
        }
    )]
    fn map_vendor_event_preserves_optional_variant_payloads(
        #[case] event: AgentStreamEvent,
        #[case] expected: RunEvent,
    ) {
        assert_eq!(map_vendor_event(event), expected);
    }

    /// Always-emitted events map field-for-field.
    #[rstest]
    #[case::run_start(
        AgentStreamEvent::RunStart { run_id: "run_1".into() },
        RunEvent::RunStart { run_id: "run_1".into() }
    )]
    #[case::text_delta(
        AgentStreamEvent::TextDelta { text: "hello".into() },
        RunEvent::TextDelta { text: "hello".into() }
    )]
    // Both text arms share one field shape; whole-event equality pins
    // the thinking variant separately.
    #[case::thinking_delta(
        AgentStreamEvent::ThinkingDelta { text: "hmm".into() },
        RunEvent::ThinkingDelta { text: "hmm".into() }
    )]
    // Call-start and call-complete share one field shape; each pinned.
    #[case::tool_call_start(
        AgentStreamEvent::ToolCallStart {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
        },
        RunEvent::ToolCallStart {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
        }
    )]
    #[case::tool_call_complete(
        AgentStreamEvent::ToolCallComplete {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
        },
        RunEvent::ToolCallComplete {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
        }
    )]
    #[case::tool_executed(
        AgentStreamEvent::ToolExecuted {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
            success: false,
            error: Some("missing".into()),
        },
        RunEvent::ToolExecuted {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
            success: false,
            error: Some("missing".into()),
        }
    )]
    #[case::error(
        AgentStreamEvent::Error { message: "boom".into() },
        RunEvent::Error { message: "boom".into() }
    )]
    #[case::cancelled(
        AgentStreamEvent::Cancelled {
            partial_text: Some("partial".into()),
            partial_thinking: None,
            pending_tools: vec!["read".into()],
        },
        RunEvent::Cancelled {
            partial_text: Some("partial".into()),
            partial_thinking: None,
            pending_tools: vec!["read".into()],
        }
    )]
    fn map_vendor_event_preserves_mapped_variant_payloads(
        #[case] event: AgentStreamEvent,
        #[case] expected: RunEvent,
    ) {
        assert_eq!(map_vendor_event(event), expected);
    }

    // ========================================================================
    // Transcript distillation
    // ========================================================================

    #[test]
    fn distill_messages_renders_tool_flow_transcript() {
        let response = ModelResponse::with_parts(vec![
            ModelResponsePart::text("checking"),
            ModelResponsePart::tool_call("read_file", json!({"path": "a.txt"})),
            // Text after the call proves the consuming pass keeps every
            // text part in order, not just the leading one.
            ModelResponsePart::text("done"),
        ]);
        let request = ModelRequest::with_parts(vec![
            ModelRequestPart::SystemPrompt(SystemPromptPart::new("sys")),
            ModelRequestPart::UserPrompt(UserPromptPart::new("read a.txt")),
            ModelRequestPart::ModelResponse(Box::new(response)),
            // Thinking carries no distilled representation; the length
            // assertion below proves it contributes no message.
            ModelRequestPart::ModelResponse(Box::new(ModelResponse::with_parts(vec![
                ModelResponsePart::thinking("internal"),
            ]))),
            ModelRequestPart::ToolReturn(
                ToolReturnPart::success("read_file", "contents").with_tool_call_id("call_1"),
            ),
            ModelRequestPart::RetryPrompt(RetryPromptPart::new("retry feedback")),
            ModelRequestPart::BuiltinToolReturn(BuiltinToolReturnPart::new(
                "web_search",
                BuiltinToolReturnContent::Other {
                    kind: "custom".into(),
                    data: json!({"hits": 1}),
                },
                "call_9",
            )),
        ]);
        // A follow-up request with a multipart user prompt covers
        // multi-request distillation and the parts-serialization branch.
        let follow_up = ModelRequest::with_parts(vec![ModelRequestPart::UserPrompt(
            UserPromptPart::new(UserContent::Parts(vec![
                serdes_ai::core::UserContentPart::text("part one"),
                serdes_ai::core::UserContentPart::text("part two"),
            ])),
        )]);

        let messages = distill_messages(vec![request, follow_up]);

        let expected = vec![
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
                text: Some("checkingdone".into()),
                tool_calls: vec![RunToolCallSummary {
                    tool_name: "read_file".into(),
                    tool_call_id: None,
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
            RunMessage {
                role: RunMessageRole::User,
                text: Some("retry feedback".into()),
                tool_calls: Vec::new(),
                tool_result: None,
            },
        ];
        assert_eq!(messages.len(), 7);
        assert_eq!(messages[..5], expected[..]);

        // The builtin tool return distills as a Tool turn whose output is
        // the serialized structured content; the exact vendor tag shape
        // stays unpinned while the payload stays observable.
        let builtin = messages[5]
            .tool_result
            .as_ref()
            .expect("builtin tool result should distill");
        assert_eq!(messages[5].role, RunMessageRole::Tool);
        assert_eq!(builtin.tool_call_id, Some("call_9".to_string()));
        assert!(
            builtin.output.contains("custom") && builtin.output.contains("hits"),
            "serialized builtin content should stay observable: {}",
            builtin.output
        );

        // The multipart prompt from the follow-up request renders as
        // serialized text so mixed content stays observable.
        assert_eq!(messages[6].role, RunMessageRole::User);
        let multipart = messages[6]
            .text
            .as_deref()
            .expect("multipart prompt should render as text");
        assert!(
            multipart.contains("part one") && multipart.contains("part two"),
            "serialized parts should stay observable: {multipart}"
        );
    }

    // ========================================================================
    // HookedAgent::run_stream integration
    // ========================================================================

    /// Run hook that records every dispatch it observes. Clones share the
    /// record so the test can register one copy and inspect another.
    #[derive(Clone)]
    struct DispatchRecorder {
        dispatches: Arc<Mutex<Vec<String>>>,
    }

    impl RunHook for DispatchRecorder {
        fn hook<'a>(
            &'a self,
            ctx: &'a HookRunContext<'a>,
            config: RunConfig,
            original: RunOriginal<'a>,
        ) -> RunHookFuture<'a> {
            self.dispatches
                .lock()
                .expect("dispatches should not be poisoned")
                .push(ctx.run_id.to_string());
            original.call(ctx, config)
        }
    }

    /// Model whose streaming requests fail before yielding any events.
    struct FailingStreamModel {
        profile: ModelProfile,
    }

    impl FailingStreamModel {
        fn new() -> Self {
            Self {
                profile: ModelProfile::default(),
            }
        }
    }

    #[async_trait::async_trait]
    impl serdes_ai_models::Model for FailingStreamModel {
        fn name(&self) -> &str {
            "failing-stream-model"
        }

        fn system(&self) -> &str {
            "test"
        }

        fn profile(&self) -> &ModelProfile {
            &self.profile
        }

        async fn request(
            &self,
            _messages: &[ModelRequest],
            _settings: &serdes_ai::core::ModelSettings,
            _params: &serdes_ai_models::ModelRequestParameters,
        ) -> Result<ModelResponse, ModelError> {
            Err(ModelError::api("upstream exploded"))
        }

        async fn request_stream(
            &self,
            _messages: &[ModelRequest],
            _settings: &serdes_ai::core::ModelSettings,
            _params: &serdes_ai_models::ModelRequestParameters,
        ) -> Result<serdes_ai_models::StreamedResponse, ModelError> {
            Err(ModelError::api("upstream exploded"))
        }
    }

    /// Builds a hooked `caller` agent with no tools, running `model`.
    fn streamed_agent(
        model: impl serdes_ai_models::Model + 'static,
        hooks: HookSet,
    ) -> HookedAgent {
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&[]),
                "prompt",
            )]))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(model);
        context.build("caller").expect("build should succeed")
    }

    /// Builds a hooked `caller` agent whose only tool is a custom `ping`
    /// tool returning "pong", running `model`.
    fn streamed_agent_with_ping_tool(
        model: impl serdes_ai_models::Model + 'static,
        hooks: HookSet,
    ) -> HookedAgent {
        let runtime = AgentRuntimeBuilder::new()
            .catalog(AgentCatalog::from_entries([agent(
                "caller",
                AgentMode::Primary,
                allow_tools(&["ping"]),
                "prompt",
            )]))
            .tools(vec![ToolCatalogEntry::new("ping", ToolCatalogKind::Custom)])
            .custom_tool(SerdesTestFactory::new(
                "ping",
                "Use ping to check connectivity.",
                "pong",
            ))
            .defaults(AgentDefaults::with_model("openrouter/openai/gpt-4.1-mini"))
            .hooks(hooks)
            .build()
            .expect("runtime should build");

        let context = AgentBuildContext::new(
            Arc::new(runtime),
            Arc::new(catalog()),
            Arc::new(credentials()),
            workspace_root(),
        )
        .with_model_override(model);
        context.build("caller").expect("build should succeed")
    }

    /// Collects every event of one `run_stream` call into owned events.
    async fn collect_events(agent: &HookedAgent, prompt: impl Into<UserContent>) -> Vec<RunEvent> {
        let mut stream = agent
            .run_stream(prompt, ())
            .await
            .expect("stream should start");
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event.expect("stream item should be ok"));
        }
        events
    }

    #[tokio::test]
    async fn run_stream_yields_incremental_text_deltas_with_run_hooks_registered() {
        const RESPONSE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let model = Streamed::new(FunctionModel::new(move |_, _| {
            ModelResponse::text(RESPONSE)
        }));
        let recorder = DispatchRecorder {
            dispatches: Arc::new(Mutex::new(Vec::new())),
        };
        let hooks = HookSet::builder().run_hook(recorder.clone()).build();
        let hooked = streamed_agent(model, hooks);

        // The multipart prompt doubles as stream-path acceptance
        // coverage: structured prompts must stream to completion, and
        // the model closure ignores prompt content.
        let prompt = UserContent::Parts(vec![
            serdes_ai::core::UserContentPart::text("hello"),
            serdes_ai::core::UserContentPart::image_url("https://example.invalid/image.png"),
        ]);
        let events = collect_events(&hooked, prompt).await;

        let delta_count = events
            .iter()
            .filter(|event| matches!(event, RunEvent::TextDelta { .. }))
            .count();
        let first_delta = events
            .iter()
            .position(|event| matches!(event, RunEvent::TextDelta { .. }));
        let complete_index = events
            .iter()
            .position(|event| matches!(event, RunEvent::RunComplete { .. }))
            .expect("run should complete");
        assert!(
            delta_count > 1,
            "expected multiple incremental text deltas, got {delta_count}"
        );
        assert!(
            first_delta < Some(complete_index),
            "a text delta must arrive before run completion"
        );

        // Registered run hooks stay inert on the streaming path.
        assert!(
            recorder
                .dispatches
                .lock()
                .expect("recorder not poisoned")
                .is_empty(),
            "run hooks must not fire on run_stream"
        );

        // Concatenated deltas equal the model's response text.
        let streamed_text: String = events
            .iter()
            .filter_map(|event| match event {
                RunEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(streamed_text, RESPONSE);
    }

    /// Stream window of the scripted ping call: the run's events plus
    /// the call's start/complete positions and stamped call id, shared
    /// by the tool-activity and optional-events tests.
    struct PingCallWindow {
        events: Vec<RunEvent>,
        call_start: usize,
        call_id: Option<String>,
        call_complete: usize,
    }

    /// Runs the scripted ping flow (the first turn calls `ping`, then
    /// the final turn answers with text) and locates the ping call's
    /// stream window.
    async fn run_scripted_ping_flow() -> PingCallWindow {
        let model = tool_then_text("ping", json!({"target": "example.com"}), "after the tool");
        let hooked = streamed_agent_with_ping_tool(model, HookSet::builder().build());
        let events = collect_events(&hooked, "use the tool").await;

        let call_start = position(&events, &|event| {
            matches!(event, RunEvent::ToolCallStart { tool_name, .. } if tool_name == "ping")
        })
        .expect("tool call start should stream");
        let call_id = match &events[call_start] {
            RunEvent::ToolCallStart { tool_call_id, .. } => tool_call_id.clone(),
            other => panic!("expected a tool call start, got {other:?}"),
        };
        let call_complete = position(&events, &|event| {
            matches!(event, RunEvent::ToolCallComplete { tool_call_id, .. }
                if tool_call_id == &call_id)
        })
        .expect("tool call complete should stream");
        PingCallWindow {
            events,
            call_start,
            call_id,
            call_complete,
        }
    }

    /// Index of the first event matching `predicate`.
    fn position(events: &[RunEvent], predicate: &dyn Fn(&RunEvent) -> bool) -> Option<usize> {
        events.iter().position(|event| predicate(event))
    }

    #[tokio::test]
    async fn run_stream_reports_tool_activity_consistent_with_transcript() {
        let PingCallWindow {
            events,
            call_start,
            call_id: streamed_call_id,
            call_complete,
        } = run_scripted_ping_flow().await;

        // The scripted mock stamps the call id; every later event and
        // transcript record must correlate on it.
        assert_eq!(
            streamed_call_id.as_deref(),
            Some("call_mock_1"),
            "the scripted call id must stream through the start event"
        );
        let executed = position(&events, &|event| {
            matches!(
                event,
                RunEvent::ToolExecuted { tool_name, tool_call_id, success, error: None, .. }
                    if tool_name == "ping" && *success && tool_call_id == &streamed_call_id
            )
        })
        .expect("successful tool execution should stream");
        assert!(
            call_start < call_complete && call_complete < executed,
            "tool events must stream in call-start, call-complete, executed order"
        );

        // Output-ready arrives after the final text but before completion.
        let output_ready = position(&events, &|event| matches!(event, RunEvent::OutputReady))
            .expect("output-ready should stream");
        let complete = position(&events, &|event| {
            matches!(event, RunEvent::RunComplete { .. })
        })
        .expect("run should complete");
        assert_eq!(
            complete,
            events.len() - 1,
            "RunComplete should be the last event"
        );
        assert!(
            output_ready < complete,
            "output-ready must precede run completion"
        );

        // The final answer streams as deltas after the tool ran.
        let streamed_answer: String = events[executed + 1..]
            .iter()
            .filter_map(|event| match event {
                RunEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        // Transcript consistency with the streamed tool activity: the
        // assistant turn records the call and arguments, the tool turn
        // records the real result, and the closing assistant turn matches
        // the streamed answer.
        let (run_id, messages) = match events.last() {
            Some(RunEvent::RunComplete { run_id, messages }) => (run_id, messages),
            other => panic!("RunComplete should be the last event, got {other:?}"),
        };
        assert!(
            events.iter().any(|event| matches!(
                event,
                RunEvent::RunStart { run_id: started } if started == run_id
            )),
            "RunComplete id must match the started run"
        );
        assert!(
            !run_id.is_empty(),
            "RunComplete must carry the inner run id"
        );
        let user_text = messages
            .iter()
            .find(|message| message.role == RunMessageRole::User)
            .and_then(|message| message.text.as_deref());
        assert_eq!(
            user_text,
            Some("use the tool"),
            "the user turn should carry the prompt verbatim"
        );
        let call_summary = messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .find(|call| call.tool_name == "ping")
            .expect("transcript should record the ping call");
        assert_eq!(
            call_summary.arguments_json.as_deref(),
            Some(r#"{"target":"example.com"}"#)
        );
        assert_eq!(
            call_summary.tool_call_id, streamed_call_id,
            "transcript call summary must carry the streamed call id"
        );
        let tool_output = messages
            .iter()
            .find(|message| message.role == RunMessageRole::Tool)
            .and_then(|message| message.tool_result.as_ref())
            .expect("transcript should record the tool result");
        assert_eq!(tool_output.output, "pong");
        assert_eq!(
            tool_output.tool_call_id, streamed_call_id,
            "transcript tool result must answer the streamed call id"
        );
        let final_answer = messages
            .iter()
            .filter(|message| message.role == RunMessageRole::Assistant)
            .filter_map(|message| message.text.as_deref())
            .last()
            .expect("closing assistant turn should carry text");
        assert_eq!(final_answer, streamed_answer);
    }

    #[tokio::test]
    async fn run_stream_surfaces_optional_events_when_backend_emits_them() {
        let PingCallWindow {
            events,
            call_start,
            call_id: streamed_call_id,
            call_complete,
        } = run_scripted_ping_flow().await;

        // Both model-request steps report start and end boundaries, so
        // consumers can group the stream by step index; this backend
        // numbers steps from one.
        let started_steps: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                RunEvent::StepStart { step } => Some(*step),
                _ => None,
            })
            .collect();
        assert_eq!(started_steps, vec![1, 2], "both steps should report starts");
        let ended_steps: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                RunEvent::StepEnd { step } => Some(*step),
                _ => None,
            })
            .collect();
        assert_eq!(ended_steps, vec![1, 2], "both steps should report ends");
        let first_step_start = position(&events, &|event| {
            matches!(event, RunEvent::StepStart { step: 1 })
        })
        .expect("first step should start");
        let first_content = position(&events, &|event| {
            matches!(
                event,
                RunEvent::TextDelta { .. } | RunEvent::ToolCallStart { .. }
            )
        })
        .expect("step content should stream");
        assert!(
            first_step_start < first_content,
            "step start must precede the step's content events"
        );
        let first_step_end = position(&events, &|event| {
            matches!(event, RunEvent::StepEnd { step: 1 })
        })
        .expect("first step should end");
        let second_step_start = position(&events, &|event| {
            matches!(event, RunEvent::StepStart { step: 2 })
        })
        .expect("second step should start");
        assert!(
            first_step_end < second_step_start,
            "steps must not interleave"
        );

        // Context telemetry arrives per model request.
        let context_infos: Vec<&RunEvent> = events
            .iter()
            .filter(|event| matches!(event, RunEvent::ContextInfo { .. }))
            .collect();
        assert_eq!(context_infos.len(), 2, "one context info per step");
        for info in context_infos {
            let RunEvent::ContextInfo { request_bytes, .. } = info else {
                unreachable!("filtered to context info");
            };
            assert!(
                *request_bytes > 0,
                "serialized request size should be positive"
            );
        }

        // Streamed argument assembly: the ping call's deltas arrive
        // between its start and completion and concatenate to the
        // call's arguments. The mock delivers arguments whole, so this
        // exercises the vendor's single-delta path.
        let streamed_args: String = events[call_start + 1..call_complete]
            .iter()
            .filter_map(|event| match event {
                RunEvent::ToolCallDelta {
                    tool_call_id,
                    delta,
                } if tool_call_id == &streamed_call_id => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            streamed_args, r#"{"target":"example.com"}"#,
            "argument deltas must concatenate to the call's arguments"
        );
    }

    #[tokio::test]
    async fn run_stream_maps_vendor_error_event_and_ends_with_inner_error() {
        let hooked = streamed_agent(FailingStreamModel::new(), HookSet::builder().build());

        let mut stream = hooked
            .run_stream("trigger the failure", ())
            .await
            .expect("stream should start");
        let mut mapped_error_message = None;
        let mut terminal = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(RunEvent::Error { message }) => mapped_error_message = Some(message),
                other => terminal = Some(other),
            }
        }

        // The vendor failure surfaces as the mapped error event first...
        let message =
            mapped_error_message.expect("vendor failure should surface as RunEvent::Error");
        assert!(
            message.contains("upstream exploded"),
            "mapped error should carry the vendor message: {message}"
        );
        // ...then the inner error terminates the stream unchanged.
        let error = terminal
            .expect("stream should end with the inner error")
            .expect_err("terminal item should be the inner error");
        assert!(
            matches!(
                error,
                serdes_ai::agent::AgentRunError::Model(ModelError::Api { .. })
            ),
            "inner model failure should keep its variant, got: {error:?}"
        );
    }
}
