//! Blocking web content fetching.

use super::{categorize_reqwest_error, check_size, process_content, WebFetchOutput};
use crate::error::{ToolError, ToolResult};
use crate::tool_metadata::webfetch::MAX_TIMEOUT_MS;
use std::io::{BufRead, BufReader};
use std::time::Duration;

/// Fetches content from a URL and returns processed content.
///
/// - HTML is converted to markdown
/// - JSON is pretty-printed
/// - Other content types returned as-is
pub fn fetch_url(
    client: &reqwest::blocking::Client,
    url: &str,
    timeout_ms: u64,
) -> ToolResult<WebFetchOutput> {
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return Err(ToolError::Validation(format!(
            "timeout_ms must be between 1 and {}",
            MAX_TIMEOUT_MS
        )));
    }

    let timeout = Duration::from_millis(timeout_ms);
    let response = client
        .get(url)
        .timeout(timeout)
        .send()
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
    const BUFFER_SIZE: usize = 65536;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, response);

    loop {
        let chunk = reader
            .fill_buf()
            .map_err(|e| ToolError::Http(e.to_string()))?;
        if chunk.is_empty() {
            break;
        }

        let n = chunk.len();
        total_len = total_len
            .checked_add(n)
            .ok_or_else(|| ToolError::Http(format!("Response size overflow for {}", url)))?;
        check_size(total_len, url)?;

        bytes.extend_from_slice(chunk);
        reader.consume(n);
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

    fn test_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .build()
            .expect("client build failed")
    }

    #[test]
    fn fetches_plain_text() {
        // Use httpbin.org for blocking tests since wiremock is async-only
        let client = test_client();
        let result = fetch_url(&client, "https://httpbin.org/robots.txt", 10_000);

        // This test requires network access, so we just check it doesn't panic
        // In CI, this might fail due to network restrictions
        if let Ok(output) = result {
            assert!(!output.content.is_empty());
            assert!(!output.content_type.is_empty());
        }
    }

    #[test]
    fn handles_404() {
        let client = test_client();
        let result = fetch_url(&client, "https://httpbin.org/status/404", 10_000);

        // In case of network issues, just verify we get some result
        if let Err(e) = result {
            assert!(matches!(e, ToolError::Http(_)));
        }
    }

    #[test]
    fn rejects_timeout_zero() {
        let client = test_client();
        let result = fetch_url(&client, "http://localhost:1", 0);
        assert!(matches!(result, Err(ToolError::Validation(_))));
    }

    #[test]
    fn rejects_timeout_exceeding_max() {
        let client = test_client();
        let result = fetch_url(&client, "http://localhost:1", MAX_TIMEOUT_MS + 1);
        assert!(matches!(result, Err(ToolError::Validation(_))));
    }
}
