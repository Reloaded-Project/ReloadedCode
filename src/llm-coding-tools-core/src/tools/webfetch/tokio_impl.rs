//! Tokio-based async web content fetching.

use super::{categorize_reqwest_error, check_size, process_content, WebFetchOutput};
use crate::error::{ToolError, ToolResult};
use std::time::Duration;

/// Fetches content from a URL and returns processed content.
///
/// - HTML is converted to markdown
/// - JSON is pretty-printed
/// - Other content types returned as-is
/// - Response size is limited to `max_response_size` bytes
///
/// # Errors
///
/// Returns `ToolError::Validation` if timeout_ms is 0 or exceeds max_timeout_ms.
pub async fn fetch_url(
    client: &reqwest::Client,
    request: super::WebFetchRequest,
    settings: super::WebFetchSettings,
) -> ToolResult<WebFetchOutput> {
    let timeout_ms = request.timeout_ms.unwrap_or(settings.default_timeout_ms());

    if timeout_ms == 0 {
        return Err(ToolError::validation_for(
            "timeout_ms",
            "timeout_ms must be at least 1",
        ));
    }
    if timeout_ms > settings.max_timeout_ms() {
        return Err(ToolError::validation_for(
            "timeout_ms",
            format!(
                "timeout_ms exceeds maximum allowed value of {}",
                settings.max_timeout_ms()
            ),
        ));
    }

    let timeout = Duration::from_millis(timeout_ms as u64);
    let mut response = client
        .get(&request.url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| categorize_reqwest_error(e, &request.url))?;

    let status = response.status();
    if !status.is_success() {
        return Err(ToolError::Http(format!(
            "HTTP {} for {}",
            status, request.url
        )));
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
                    len, request.url
                ))
            })
        })
        .transpose()?;
    if let Some(len) = content_length {
        check_size(len, &request.url, settings.max_response_size())?;
    }

    // Stream response body with incremental size checks to avoid memory exhaustion
    let mut bytes = content_length.map_or_else(Vec::new, Vec::with_capacity);
    let mut total_len: usize = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| ToolError::Http(e.to_string()))?
    {
        total_len = total_len.checked_add(chunk.len()).ok_or_else(|| {
            ToolError::Http(format!("Response size overflow for {}", request.url))
        })?;
        check_size(total_len, &request.url, settings.max_response_size())?;
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
    use crate::tools::webfetch::WebFetchSettings;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_settings() -> WebFetchSettings {
        WebFetchSettings::new()
            .with_timeouts(5_000, 10_000)
            .unwrap()
    }

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
            super::super::WebFetchRequest {
                url: format!("{}/text", server.uri()),
                timeout_ms: None,
            },
            test_settings(),
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
            super::super::WebFetchRequest {
                url: format!("{}/html", server.uri()),
                timeout_ms: None,
            },
            test_settings(),
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
            super::super::WebFetchRequest {
                url: format!("{}/json", server.uri()),
                timeout_ms: None,
            },
            test_settings(),
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
            super::super::WebFetchRequest {
                url: format!("{}/notfound", server.uri()),
                timeout_ms: None,
            },
            test_settings(),
        )
        .await;

        assert!(matches!(result, Err(ToolError::Http(_))));
    }

    #[tokio::test]
    async fn rejects_timeout_zero() {
        let client = test_client();
        let result = fetch_url(
            &client,
            super::super::WebFetchRequest {
                url: "http://localhost:1".to_string(),
                timeout_ms: Some(0),
            },
            test_settings(),
        )
        .await;
        assert!(matches!(result, Err(ToolError::Validation { .. })));
    }

    #[tokio::test]
    async fn rejects_timeout_exceeding_max() {
        let client = test_client();
        let result = fetch_url(
            &client,
            super::super::WebFetchRequest {
                url: "http://localhost:1".to_string(),
                timeout_ms: Some(11_000),
            },
            test_settings(),
        )
        .await;
        assert!(matches!(result, Err(ToolError::Validation { .. })));
    }
}
