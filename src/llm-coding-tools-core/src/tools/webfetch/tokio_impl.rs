//! Tokio-based async web content fetching.

use super::{categorize_reqwest_error, check_size, process_content, WebFetchOutput};
use crate::error::{ToolError, ToolResult};
use std::time::Duration;

/// Fetches content from a URL and returns processed content.
///
/// - HTML is converted to markdown
/// - JSON is pretty-printed
/// - Other content types returned as-is
pub async fn fetch_url(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> ToolResult<WebFetchOutput> {
    let mut response = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| categorize_reqwest_error(e, url))?;

    let status = response.status();
    if !status.is_success() {
        return Err(ToolError::Http(format!("HTTP {} for {}", status, url)));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/plain")
        .to_string();

    // Check Content-Length header if available for early rejection and preallocation
    let content_length = response
        .content_length()
        .map(|len| {
            usize::try_from(len).map_err(|_| {
                ToolError::Http(format!(
                    "Content-Length {} exceeds platform limits for {}",
                    len, url
                ))
            })
        })
        .transpose()?;
    if let Some(len) = content_length {
        check_size(len, url)?;
    }

    // Stream response body with incremental size checks to avoid memory exhaustion
    let mut bytes = content_length.map_or_else(Vec::new, Vec::with_capacity);
    let mut total_len: usize = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| ToolError::Http(e.to_string()))?
    {
        total_len += chunk.len();
        check_size(total_len, url)?;
        bytes.extend_from_slice(&chunk);
    }

    let byte_length = total_len;
    let raw_content = String::from_utf8_lossy(&bytes);
    let content = process_content(&raw_content, &content_type);

    Ok(WebFetchOutput {
        content,
        content_type,
        byte_length,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .build()
            .expect("client build failed")
    }

    #[tokio::test]
    async fn fetches_plain_text() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/text"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes("Hello, world!")
                    .insert_header("content-type", "text/plain"),
            )
            .mount(&server)
            .await;

        let client = test_client();
        let result = fetch_url(
            &client,
            &format!("{}/text", server.uri()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        assert!(result.content.contains("Hello, world!"));
        assert!(result.content_type.contains("text/plain"));
    }

    #[tokio::test]
    async fn converts_html_to_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/html"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes("<h1>Hello</h1><p>World</p>")
                    .insert_header("content-type", "text/html"),
            )
            .mount(&server)
            .await;

        let client = test_client();
        let result = fetch_url(
            &client,
            &format!("{}/html", server.uri()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        assert!(result.content.contains("Hello"));
        assert!(!result.content.contains("<h1>"));
    }

    #[tokio::test]
    async fn formats_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"key":"value"})),
            )
            .mount(&server)
            .await;

        let client = test_client();
        let result = fetch_url(
            &client,
            &format!("{}/json", server.uri()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

        assert!(result.content.contains("\"key\""));
    }

    #[tokio::test]
    async fn handles_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/notfound"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = test_client();
        let result = fetch_url(
            &client,
            &format!("{}/notfound", server.uri()),
            Duration::from_secs(5),
        )
        .await;

        assert!(matches!(result, Err(ToolError::Http(_))));
    }
}
