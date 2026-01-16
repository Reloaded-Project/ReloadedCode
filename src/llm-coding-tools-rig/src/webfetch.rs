//! Web content fetching tool.
//!
//! Provides URL fetching with format conversion support.

use llm_coding_tools_core::operations::fetch_url;
use llm_coding_tools_core::{ToolContext, ToolError, ToolOutput};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::time::Duration;

/// Default timeout: 30 seconds.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// Arguments for the webfetch tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct WebFetchArgs {
    /// The URL to fetch.
    pub url: String,
    /// Timeout in milliseconds (default: 30000).
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
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

impl Tool for WebFetchTool {
    const NAME: &'static str = "WebFetch";

    type Error = ToolError;
    type Args = WebFetchArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: <Self as Tool>::NAME.to_string(),
            description:
                "Fetch content from a URL. HTML is converted to markdown, JSON is prettified."
                    .to_string(),
            parameters: serde_json::to_value(schema_for!(WebFetchArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let timeout = Duration::from_millis(args.timeout_ms);
        let result = fetch_url(&self.client, &args.url, timeout).await?;
        Ok(result.into())
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
}
