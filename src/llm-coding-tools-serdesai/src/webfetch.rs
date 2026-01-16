//! Web content fetching tool.
//!
//! Provides URL fetching with format conversion support.

use crate::convert::to_serdes_result;
use crate::schema::webfetch_schema;
use async_trait::async_trait;
use llm_coding_tools_core::ToolOutput;
use llm_coding_tools_core::context::ToolContext;
use llm_coding_tools_core::operations::fetch_url;
use serde::Deserialize;
use serdes_ai::tools::{RunContext, Tool, ToolDefinition, ToolError, ToolResult};
use std::time::Duration;

/// Default timeout: 30 seconds.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// Arguments for the webfetch tool.
#[derive(Debug, Clone, Deserialize)]
struct WebFetchArgs {
    /// The URL to fetch.
    url: String,
    /// Timeout in milliseconds (default: 30000).
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

/// Tool for fetching web content.
///
/// - HTML is converted to markdown
/// - JSON is pretty-printed
/// - Other content returned as-is
#[derive(Debug, Clone)]
pub struct WebFetchTool {
    client: reqwest::Client,
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchTool {
    /// Creates a new webfetch tool with default client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Creates a webfetch tool with a custom client.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl<Deps: Send + Sync> Tool<Deps> for WebFetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "WebFetch",
            "Fetch content from a URL. HTML is converted to markdown, JSON is prettified.",
        )
        .with_parameters(webfetch_schema().expect("schema serialization should never fail"))
    }

    async fn call(&self, _ctx: &RunContext<Deps>, args: serde_json::Value) -> ToolResult {
        let args: WebFetchArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::validation_error("WebFetch", None, e.to_string()))?;
        let timeout = Duration::from_millis(args.timeout_ms);
        let result = fetch_url(&self.client, &args.url, timeout).await;

        to_serdes_result("WebFetch", result.map(ToolOutput::from))
    }
}

impl ToolContext for WebFetchTool {
    const NAME: &'static str = "WebFetch";

    fn context(&self) -> &'static str {
        llm_coding_tools_core::context::WEBFETCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_ctx() -> RunContext<()> {
        RunContext::minimal("test-model")
    }

    #[test]
    fn creates_with_default_client() {
        let _tool = WebFetchTool::new();
    }

    #[test]
    fn creates_with_custom_client() {
        let client = reqwest::Client::builder()
            .user_agent("test")
            .build()
            .unwrap();
        let _tool = WebFetchTool::with_client(client);
    }

    #[tokio::test]
    async fn fetches_url_with_wiremock() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes("<html><body><h1>Hello</h1></body></html>")
                    .insert_header("content-type", "text/html"),
            )
            .mount(&mock_server)
            .await;

        let tool = WebFetchTool::new();
        let args = serde_json::json!({
            "url": format!("{}/test", mock_server.uri()),
            "timeout_ms": 5000
        });

        let result = tool.call(&mock_ctx(), args).await.unwrap();
        let text = result.as_text().unwrap();

        // Should contain content type info and converted content
        assert!(text.contains("text/html"));
        assert!(text.contains("Hello"));
    }
}
