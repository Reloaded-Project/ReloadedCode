//! Mock model types with streaming support for running agents without a real LLM provider.
//!
//! Wraps upstream [`serdes_ai_models`] mock types so they work with
//! [`Agent::run_stream`][`serdes_ai::Agent::run_stream`].
//!
//! # Quick start
//!
//! ```text
//! use reloaded_code_serdesai::mock::{Streamed, tool_then_text};
//! use serde_json::json;
//!
//! let model = tool_then_text("glob", json!({"pattern": "*.rs"}), "Done.");
//! let stream = agent.run_stream("prompt", ()).await?; // OK
//! ```
//!
//! When using [`crate::AgentBuildContext`], call
//! [`with_model_override`]
//! to inject the mock model before calling [`build()`].
//!
//! [`with_model_override`]: crate::AgentBuildContext::with_model_override
//! [`build()`]: crate::AgentBuildContext::build

// Re-export upstream mock types so users can still access the raw variants when needed.
use async_trait::async_trait;
use futures::stream;
use serdes_ai::core::{
    FinishReason, ModelRequest, ModelResponse, ModelResponsePart, ModelResponseStreamEvent,
};
use serdes_ai_models::Model as ModelTrait;
pub use serdes_ai_models::{FunctionModel, MockModel, TestModel};
// Re-export the types from where serdes-ai-models exposes them.
use serdes_ai::core::ModelSettings;
use serdes_ai_models::{
    ModelCapability, ModelError, ModelProfile, ModelRequestParameters, StreamedResponse,
};

// ============================================================================
// Streamed - wrapper that adds streaming support to any Model
// ============================================================================

/// Wrapper adding [`request_stream`] support to any [`ModelTrait`] implementation.
///
/// Delegates [`request`] directly to the inner model and converts the non-streaming
/// response into a sequence of [`ModelResponseStreamEvent`]s for streaming callers.
///
/// # Example
///
/// ```rust,no_run
/// use reloaded_code_serdesai::mock::{FunctionModel, Streamed};
/// use serde_json::json;
///
/// let model = Streamed::new(FunctionModel::tool_call("glob", json!({"pattern": "*.rs"})));
/// ```
///
/// [`request_stream`]: ModelTrait::request_stream
/// [`request`]: ModelTrait::request
#[derive(Clone, Debug)]
pub struct Streamed<T> {
    inner: T,
    name: String,
}

impl<T> Streamed<T> {
    /// Wrap a model to add streaming support.
    ///
    /// The `name` defaults to the inner model's [`name()`].
    ///
    /// [`name()`]: ModelTrait::name
    pub fn new(inner: T) -> Self
    where
        T: ModelTrait,
    {
        let name = inner.name().to_string();
        Self { inner, name }
    }

    /// Set a custom name for the wrapped model.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }
}

#[async_trait]
impl<T: ModelTrait + Send + Sync> ModelTrait for Streamed<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn system(&self) -> &str {
        self.inner.system()
    }

    fn profile(&self) -> &ModelProfile {
        self.inner.profile()
    }

    async fn request(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<ModelResponse, ModelError> {
        self.inner.request(messages, settings, params).await
    }

    async fn request_stream(
        &self,
        messages: &[ModelRequest],
        settings: &ModelSettings,
        params: &ModelRequestParameters,
    ) -> Result<StreamedResponse, ModelError> {
        let response = self.inner.request(messages, settings, params).await?;
        let events = response_to_stream_events(response);
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }

    fn supports(&self, capability: ModelCapability) -> bool {
        self.inner.supports(capability)
    }
}

// ============================================================================
// Convenience helpers
// ============================================================================

/// Build a mock model that calls `tool_name` with `args` on the **first** turn,
/// then returns text that incorporates the real tool return on the **second** turn.
///
/// This prevents infinite loops when running agent examples that stream,
/// because after the tool result is fed back the model answers with text.
///
/// The second-turn response includes whatever the real tool returned, so
/// the output reflects actual tool execution rather than a canned message.
///
/// # Arguments
///
/// - `tool_name` - name of the tool to call on the first turn.
/// - `args` - JSON arguments passed to the tool call on the first turn.
/// - `fallback_text` - text prefix for the second-turn response; the real tool
///   return is appended after it.
///
/// # Example
///
/// ```rust,no_run
/// use reloaded_code_serdesai::mock::tool_then_text;
/// use serde_json::json;
///
/// let model = tool_then_text("glob", json!({"pattern": "*.rs"}), "Done.");
/// ```
pub fn tool_then_text(
    tool_name: impl Into<String>,
    args: serde_json::Value,
    fallback_text: impl Into<String>,
) -> Streamed<FunctionModel> {
    let tool_name = tool_name.into();
    let fallback_text = fallback_text.into();

    let model = FunctionModel::new(move |messages, _settings| {
        // Check whether the conversation already contains a tool return from a
        // previous turn.  If it does, we are on the second call and should
        // produce a text response incorporating the real result.
        let has_tool_return = messages.iter().any(|m| {
            m.parts
                .iter()
                .any(|p| matches!(p, serdes_ai::core::ModelRequestPart::ToolReturn(_)))
        });

        if has_tool_return {
            // Collect tool return content from the message history.
            let tool_results: String = messages
                .iter()
                .flat_map(|m| m.tool_returns())
                .map(extract_tool_return_text)
                .collect::<Vec<_>>()
                .join("\n");

            let text = if tool_results.is_empty() {
                fallback_text.clone()
            } else {
                format!("{fallback_text}\n\n{tool_results}")
            };

            ModelResponse::text(text)
        } else {
            // First call: emit a tool call so the agent executes the real tool.
            tool_call_response(&tool_name, &args)
        }
    });

    Streamed::new(model)
}

/// Scripts two tool calls followed by a text answer, generalising
/// [`tool_then_text`] to two-step flows: the first turn calls `first`, the
/// next turn calls `second`, and once both tool returns are in the
/// conversation the model answers with `final_text` followed by the real
/// tool returns, so callers can observe what the model received.
///
/// # Arguments
///
/// - `first` - name and JSON arguments of the tool called on the first turn.
/// - `second` - name and JSON arguments of the tool called on the second turn.
/// - `final_text` - text prefix for the closing response; the real tool
///   returns are appended after it.
///
/// # Example
///
/// ```rust,no_run
/// use reloaded_code_serdesai::mock::two_tools_then_text;
/// use serde_json::json;
///
/// let model = two_tools_then_text(
///     ("read", json!({"file_path": "service.env"})),
///     ("write", json!({"file_path": "draft.md", "content": "notes"})),
///     "Run finished.",
/// );
/// ```
pub fn two_tools_then_text(
    first: (impl Into<String>, serde_json::Value),
    second: (impl Into<String>, serde_json::Value),
    final_text: impl Into<String>,
) -> Streamed<FunctionModel> {
    let first_name = first.0.into();
    let first_args = first.1;
    let second_name = second.0.into();
    let second_args = second.1;
    let final_text = final_text.into();

    let model = FunctionModel::new(move |messages, _settings| {
        // Each finished tool call adds one tool return to the history, so
        // their count tells which scripted turn is next.
        let answered_calls = messages.iter().flat_map(|m| m.tool_returns()).count();

        match answered_calls {
            0 => tool_call_response(&first_name, &first_args),
            1 => tool_call_response(&second_name, &second_args),
            _ => {
                let tool_results: String = messages
                    .iter()
                    .flat_map(|m| m.tool_returns())
                    .map(extract_tool_return_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if tool_results.is_empty() {
                    final_text.clone()
                } else {
                    format!("{final_text}\n\n{tool_results}")
                };
                ModelResponse::text(text)
            }
        }
    });

    Streamed::new(model)
}

/// Extract human-readable text from a [`ToolReturnPart`].
///
/// Uses serde JSON round-tripping to avoid depending on the
/// non-public `ToolReturnContent` enum variants directly.
fn extract_tool_return_text(tr: &serdes_ai::core::ToolReturnPart) -> String {
    // Serialize the content field to JSON, then extract readable text.
    // ToolReturnContent variants produce:
    //   Text    -> {"type":"text","content":"..."}
    //   Json    -> {"type":"json","content":{...}}
    //   Error   -> {"type":"error","message":"..."}
    //   Multiple-> {"type":"multiple","items":[...]}
    //   Image   -> {"type":"image","image":{...}}
    let Ok(val) = serde_json::to_value(&tr.content) else {
        return format!("{:?}", tr.content);
    };

    // Try text content field first (most common case).
    if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }

    // JSON content field.
    if let Some(json_val) = val.get("content")
        && let Ok(pretty) = serde_json::to_string_pretty(json_val)
    {
        return pretty;
    }

    // Error message field.
    if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
        return format!("[error] {msg}");
    }

    // Fallback: pretty-print the whole thing.
    serde_json::to_string_pretty(&val).unwrap_or_else(|_| format!("{:?}", tr.content))
}

// ============================================================================
// Private helpers
// ============================================================================

fn response_to_stream_events(response: ModelResponse) -> Vec<ModelResponseStreamEvent> {
    let mut events = Vec::with_capacity(response.parts.len() * 2 + 1);

    for (index, part) in response.parts.into_iter().enumerate() {
        events.push(ModelResponseStreamEvent::part_start(index, part));
        events.push(ModelResponseStreamEvent::part_end(index));
    }

    events
}

/// Emits a tool-call response: a short text part, then the call that
/// triggers the real tool.
fn tool_call_response(tool_name: &str, args: &serde_json::Value) -> ModelResponse {
    ModelResponse::with_parts(vec![
        ModelResponsePart::text(format!("Calling {tool_name}...")),
        ModelResponsePart::tool_call(tool_name, args.clone()),
    ])
    .with_finish_reason(FinishReason::ToolCall)
}
