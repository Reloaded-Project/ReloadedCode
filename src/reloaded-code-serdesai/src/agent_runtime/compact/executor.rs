//! Summarize port over the run's own model.

use reloaded_code_core::{SummaryExecutor, SummaryFuture, SummaryRequest, ToolError};
use serdes_ai::core::{
    ModelRequest, ModelRequestPart, ModelResponse, ModelResponsePart, ModelSettings,
    SystemPromptPart, UserContent, UserPromptPart,
};
use serdes_ai_models::{BoxedModel, ModelRequestParameters};

/// Port running summarize requests on the run's own model.
///
/// Each call copies the run's [`ModelSettings`] with
/// [`SummaryRequest::max_output_tokens`] as the output-token cap, so
/// the summary budget rides on the request itself and the run's own
/// provider serves it.
pub(super) struct ModelSummaryExecutor<'a> {
    /// Wrapped model serving summarize requests.
    model: &'a BoxedModel,
    /// The run's model settings; each call copies them with the
    /// request's output-token cap.
    settings: &'a ModelSettings,
}

impl<'a> ModelSummaryExecutor<'a> {
    /// Assembles the executor over the run's model and settings.
    pub(super) fn new(model: &'a BoxedModel, settings: &'a ModelSettings) -> Self {
        Self { model, settings }
    }
}

impl SummaryExecutor for ModelSummaryExecutor<'_> {
    fn summarize<'a>(&'a self, request: SummaryRequest) -> SummaryFuture<'a> {
        // The cap is policy-clamped already; the settings copy keeps
        // the run's sampling parameters.
        let mut settings = self.settings.clone();
        settings.max_tokens = Some(request.max_output_tokens);
        let messages = vec![ModelRequest::with_parts(vec![
            ModelRequestPart::SystemPrompt(SystemPromptPart::new(request.system_prompt)),
            ModelRequestPart::UserPrompt(UserPromptPart::new(UserContent::Text(
                request.transcript,
            ))),
        ])];
        let model = self.model;
        Box::pin(async move {
            let response = model
                .request(&messages, &settings, &ModelRequestParameters::default())
                .await
                .map_err(|error| ToolError::Execution(error.to_string()))?;
            Ok(summary_text(&response))
        })
    }
}

/// Joins the text parts of one summarize response.
///
/// The Core planner rejects an empty summary itself, so the joined
/// text is returned as-is.
fn summary_text(response: &ModelResponse) -> String {
    // One sizing scan gives the buffer exact capacity.
    let text_len: usize = response
        .parts
        .iter()
        .map(|part| match part {
            ModelResponsePart::Text(text) => text.content.len(),
            _ => 0,
        })
        .sum();
    let mut summary = String::with_capacity(text_len);
    for part in &response.parts {
        if let ModelResponsePart::Text(text) = part {
            summary.push_str(&text.content);
        }
    }
    summary
}
