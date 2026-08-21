//! Executor port running the summarize request on the run's model.

use crate::ToolError;
use std::future::Future;
use std::pin::Pin;

/// Boxed future returned by [`SummaryExecutor::summarize`].
pub type SummaryFuture<'a> = Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>>;

/// One summarize request handed through the executor port.
///
/// The planner builds it; the executor maps it onto the run's model.
#[derive(Debug, Clone)]
pub struct SummaryRequest {
    /// System prompt directing the summarization.
    pub system_prompt: &'static str,
    /// Transcript text to summarize.
    pub transcript: String,
    /// Maximum output tokens for the request, already clamped to the
    /// model's advertised limit.
    pub max_output_tokens: u64,
}

/// Port executing one summarize request on the run's model.
///
/// Implemented by the runtime wiring so the planner stays free of
/// model and vendor knowledge. Implementations send the request to
/// the run's own provider with [`SummaryRequest::max_output_tokens`]
/// as the request's output-token cap and return its text.
pub trait SummaryExecutor: Send + Sync {
    /// Runs one summarize request, returning the summary text.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the request fails or yields no
    /// usable text.
    fn summarize<'a>(&'a self, request: SummaryRequest) -> SummaryFuture<'a>;
}
