//! Vendor stream events mapped to framework-owned run events.
//!
//! [`RunEventStream`] wraps the vendor [`AgentStream`] and lazily maps each
//! [`AgentStreamEvent`] to a [`RunEvent`] as the consumer polls it. SerdesAI
//! event types stay inside this module; consumers of
//! [`HookedAgent::run_stream`][task] only ever see [`RunEvent`] items.
//!
//! # Dropped vendor-only events
//!
//! The vendor emits five events with no [`RunEvent`] counterpart; the
//! mapping drops them:
//!
//! - `ContextInfo` and `ContextCompressed`: context-size telemetry emitted
//!   before each model request, and compression notices. Context metrics
//!   are not surfaced anywhere else.
//! - `RequestStart` and `ResponseComplete`: model-request step boundaries.
//!   Dropping them removes the step index consumers could use to group
//!   events by model request.
//! - `ToolCallDelta`: incremental tool-call argument fragments. Dropping it
//!   removes streamed argument assembly; complete arguments remain
//!   available in the [`RunEvent::RunComplete`] transcript as
//!   [`RunToolCallSummary::arguments_json`].
//!
//! Observable information loss: step boundaries and incremental tool-call
//! arguments no longer stream, and context telemetry is not surfaced.
//!
//! [task]: super::task::HookedAgent::run_stream

use futures::{Stream, StreamExt};
use reloaded_code_core::hooks::{
    RunEvent, RunMessage, RunMessageRole, RunToolCallSummary, RunToolResultSummary,
};
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
/// handle is added on this side. Vendor-only events (see the module docs)
/// are dropped rather than yielded.
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
        // Dropped events yield nothing, so keep polling until a mappable
        // event, an error, or the stream's end arrives.
        let inner = &mut self.get_mut().inner;
        loop {
            match inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    if let Some(event) = map_vendor_event(event) {
                        return Poll::Ready(Some(Ok(event)));
                    }
                }
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Maps one vendor event to its framework-owned counterpart.
///
/// Returns `None` for vendor-only events; see the module docs for the
/// dropped set and its observable information loss.
fn map_vendor_event(event: AgentStreamEvent) -> Option<RunEvent> {
    Some(match event {
        AgentStreamEvent::RunStart { run_id } => RunEvent::RunStart { run_id },
        AgentStreamEvent::TextDelta { text } => RunEvent::TextDelta { text },
        AgentStreamEvent::ThinkingDelta { text } => RunEvent::ThinkingDelta { text },
        AgentStreamEvent::ToolCallStart {
            tool_name,
            tool_call_id,
        } => RunEvent::ToolCallStart {
            tool_name,
            tool_call_id,
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
        AgentStreamEvent::ContextInfo { .. }
        | AgentStreamEvent::ContextCompressed { .. }
        | AgentStreamEvent::RequestStart { .. }
        | AgentStreamEvent::ToolCallDelta { .. }
        | AgentStreamEvent::ResponseComplete { .. } => return None,
    })
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
                    distilled.push(authored_message(
                        RunMessageRole::User,
                        part.content.message().to_owned(),
                    ));
                }
                ModelRequestPart::ToolReturn(part) => {
                    distilled.push(tool_result_message(
                        part.tool_call_id,
                        part.content.to_string_content(),
                    ));
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
    let text = response.text_content();
    let text = (!text.is_empty()).then_some(text);
    let mut tool_calls = Vec::new();
    for part in &response.parts {
        if let ModelResponsePart::ToolCall(call) = part {
            tool_calls.push(RunToolCallSummary {
                tool_name: call.tool_name.clone(),
                tool_call_id: call.tool_call_id.clone(),
                arguments_json: call.args.to_json_string().ok(),
            });
        }
    }
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

    #[test]
    fn map_vendor_event_drops_vendor_only_variants() {
        let dropped = vec![
            AgentStreamEvent::ContextInfo {
                estimated_tokens: 1,
                request_bytes: 4,
                context_limit: None,
            },
            AgentStreamEvent::ContextCompressed {
                original_tokens: 10,
                compressed_tokens: 5,
                strategy: "truncate".into(),
                messages_before: 2,
                messages_after: 1,
            },
            AgentStreamEvent::RequestStart { step: 1 },
            AgentStreamEvent::ToolCallDelta {
                delta: "{\"a\":".into(),
                tool_call_id: Some("call_1".into()),
            },
            AgentStreamEvent::ResponseComplete { step: 1 },
        ];
        for event in dropped {
            assert!(
                map_vendor_event(event).is_none(),
                "vendor-only event should be dropped"
            );
        }
    }

    #[test]
    fn map_vendor_event_preserves_mapped_variant_payloads() {
        let Some(RunEvent::Error { message }) = map_vendor_event(AgentStreamEvent::Error {
            message: "boom".into(),
        }) else {
            panic!("error event should map");
        };
        assert_eq!(message, "boom");

        let Some(RunEvent::Cancelled {
            partial_text,
            partial_thinking,
            pending_tools,
        }) = map_vendor_event(AgentStreamEvent::Cancelled {
            partial_text: Some("partial".into()),
            partial_thinking: None,
            pending_tools: vec!["read".into()],
        })
        else {
            panic!("cancelled event should map");
        };
        assert_eq!(partial_text.as_deref(), Some("partial"));
        assert!(partial_thinking.is_none());
        assert_eq!(pending_tools, vec!["read".to_string()]);

        // A mislabel of the thinking arm (both sides are bare text records)
        // would compile, so pin the variant identity here.
        let Some(RunEvent::ThinkingDelta { text }) =
            map_vendor_event(AgentStreamEvent::ThinkingDelta { text: "hmm".into() })
        else {
            panic!("thinking delta should map to the thinking variant");
        };
        assert_eq!(text, "hmm");

        // The call-start and call-complete arms share one field shape, so
        // pin each variant identity and its id separately.
        let Some(RunEvent::ToolCallStart {
            tool_name,
            tool_call_id,
        }) = map_vendor_event(AgentStreamEvent::ToolCallStart {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
        })
        else {
            panic!("tool call start should map to the start variant");
        };
        assert_eq!(
            (tool_name.as_str(), tool_call_id.as_deref()),
            ("read", Some("call_1"))
        );

        let Some(RunEvent::ToolCallComplete {
            tool_name,
            tool_call_id,
        }) = map_vendor_event(AgentStreamEvent::ToolCallComplete {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
        })
        else {
            panic!("tool call complete should map to the complete variant");
        };
        assert_eq!(
            (tool_name.as_str(), tool_call_id.as_deref()),
            ("read", Some("call_1"))
        );

        let Some(RunEvent::ToolExecuted {
            tool_name,
            tool_call_id,
            success,
            error,
        }) = map_vendor_event(AgentStreamEvent::ToolExecuted {
            tool_name: "read".into(),
            tool_call_id: Some("call_1".into()),
            success: false,
            error: Some("missing".into()),
        })
        else {
            panic!("tool executed event should map");
        };
        assert_eq!(tool_name, "read");
        assert_eq!(tool_call_id.as_deref(), Some("call_1"));
        assert!(!success);
        assert_eq!(error.as_deref(), Some("missing"));
    }

    // ========================================================================
    // Transcript distillation
    // ========================================================================

    #[test]
    fn distill_messages_renders_tool_flow_transcript() {
        let response = ModelResponse::with_parts(vec![
            ModelResponsePart::text("checking"),
            ModelResponsePart::tool_call("read_file", json!({"path": "a.txt"})),
        ]);
        let request = ModelRequest::with_parts(vec![
            ModelRequestPart::SystemPrompt(SystemPromptPart::new("sys")),
            ModelRequestPart::UserPrompt(UserPromptPart::new("read a.txt")),
            ModelRequestPart::ModelResponse(Box::new(response)),
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

        let messages = distill_messages(vec![request]);

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
                text: Some("checking".into()),
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
        assert_eq!(messages.len(), 6);
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
    }

    #[test]
    fn distill_messages_serializes_multipart_user_prompts() {
        let request = ModelRequest::with_parts(vec![ModelRequestPart::UserPrompt(
            UserPromptPart::new(UserContent::Parts(vec![
                serdes_ai::core::UserContentPart::text("part one"),
                serdes_ai::core::UserContentPart::text("part two"),
            ])),
        )]);

        let messages = distill_messages(vec![request]);

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, RunMessageRole::User);
        let text = messages[0]
            .text
            .as_deref()
            .expect("multipart prompt should render as text");
        assert!(
            text.contains("part one") && text.contains("part two"),
            "serialized parts should stay observable: {text}"
        );
    }

    #[test]
    fn distill_messages_skips_responses_without_distillable_content() {
        let response = ModelResponse::with_parts(vec![ModelResponsePart::thinking("internal")]);
        let request =
            ModelRequest::with_parts(vec![ModelRequestPart::ModelResponse(Box::new(response))]);

        assert!(distill_messages(vec![request]).is_empty());
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

        let events = collect_events(&hooked, "hello").await;

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

    #[tokio::test]
    async fn run_stream_run_complete_carries_real_run_id_and_faithful_transcript() {
        // The model echoes the last user prompt so the transcript ties to
        // the prompt text.
        let model = Streamed::new(FunctionModel::new(|messages, _| {
            let prompt = messages
                .iter()
                .rev()
                .flat_map(|message| message.user_prompts())
                .next()
                .and_then(|prompt| prompt.content.as_text())
                .unwrap_or_default()
                .to_string();
            ModelResponse::text(format!("echo: {prompt}"))
        }));
        let hooked = streamed_agent(model, HookSet::builder().build());

        let events = collect_events(&hooked, "transcript probe").await;

        let mut start_run_id = None;
        let mut streamed_text = String::new();
        let mut complete = None;
        for event in events {
            match event {
                RunEvent::RunStart { run_id } => start_run_id = Some(run_id),
                RunEvent::TextDelta { text } => streamed_text.push_str(&text),
                RunEvent::RunComplete { run_id, messages } => {
                    complete = Some((run_id, messages));
                }
                _ => {}
            }
        }
        let (run_id, messages) = complete.expect("run should complete");
        assert!(
            !run_id.is_empty(),
            "RunComplete must carry the inner run id"
        );
        assert_eq!(
            start_run_id.as_deref(),
            Some(run_id.as_str()),
            "RunComplete id must match the started run"
        );

        // Transcript consistency: the user turn carries the prompt, the
        // assistant turn carries exactly what was streamed.
        let user_text = messages
            .iter()
            .find(|message| message.role == RunMessageRole::User)
            .and_then(|message| message.text.as_deref());
        assert_eq!(user_text, Some("transcript probe"));
        let assistant_text = messages
            .iter()
            .find(|message| message.role == RunMessageRole::Assistant)
            .and_then(|message| message.text.as_deref());
        assert_eq!(assistant_text, Some(streamed_text.as_str()));
    }

    #[tokio::test]
    async fn run_stream_accepts_multipart_prompt() {
        let model = Streamed::new(FunctionModel::new(|_, _| ModelResponse::text("handled")));
        let hooked = streamed_agent(model, HookSet::builder().build());

        let prompt = UserContent::Parts(vec![
            serdes_ai::core::UserContentPart::text("describe this"),
            serdes_ai::core::UserContentPart::image_url("https://example.invalid/image.png"),
        ]);

        let events = collect_events(&hooked, prompt).await;

        assert!(
            events
                .iter()
                .any(|event| matches!(event, RunEvent::RunComplete { .. })),
            "multipart prompt should stream to completion"
        );
    }

    #[tokio::test]
    async fn run_stream_reports_tool_activity_consistent_with_transcript() {
        // Scripted flow: the first turn calls `ping`, then the final turn
        // answers with text once the tool return is in the history.
        let model = tool_then_text("ping", json!({"target": "example.com"}), "after the tool");
        let hooked = streamed_agent_with_ping_tool(model, HookSet::builder().build());

        let events = collect_events(&hooked, "use the tool").await;

        let position = |predicate: &dyn Fn(&RunEvent) -> bool| {
            events.iter().position(|event| predicate(event))
        };
        let call_start = position(&|event| {
            matches!(event, RunEvent::ToolCallStart { tool_name, .. } if tool_name == "ping")
        })
        .expect("tool call start should stream");
        // The scripted mock stamps the call id; every later event and
        // transcript record must correlate on it.
        let streamed_call_id = match &events[call_start] {
            RunEvent::ToolCallStart { tool_call_id, .. } => tool_call_id.clone(),
            other => panic!("expected a tool call start, got {other:?}"),
        };
        assert_eq!(
            streamed_call_id.as_deref(),
            Some("call_mock"),
            "the scripted call id must stream through the start event"
        );
        let call_complete = position(&|event| {
            matches!(event, RunEvent::ToolCallComplete { tool_call_id, .. }
                if tool_call_id == &streamed_call_id)
        })
        .expect("tool call complete should stream");
        let executed = position(&|event| {
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
        let output_ready = position(&|event| matches!(event, RunEvent::OutputReady))
            .expect("output-ready should stream");
        let complete = position(&|event| matches!(event, RunEvent::RunComplete { .. }))
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
