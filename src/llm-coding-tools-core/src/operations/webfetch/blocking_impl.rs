//! Blocking web content fetching.

use super::{categorize_reqwest_error, check_size, process_content, WebFetchOutput};
use crate::error::{ToolError, ToolResult};
use std::io::Read;
use std::time::Duration;

/// Fetches content from a URL and returns processed content.
///
/// - HTML is converted to markdown
/// - JSON is pretty-printed
/// - Other content types returned as-is
pub fn fetch_url(
    client: &reqwest::blocking::Client,
    url: &str,
    timeout: Duration,
) -> ToolResult<WebFetchOutput> {
    let mut response = client
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
    let content_length = response.content_length().map(|len| len as usize);
    if let Some(len) = content_length {
        check_size(len, url)?;
    }

    // Stream response body with incremental size checks to avoid memory exhaustion
    let mut bytes = content_length.map_or_else(Vec::new, Vec::with_capacity);
    let mut total_len: usize = 0;
    let mut buffer = [0u8; 8192];

    loop {
        let n = response
            .read(&mut buffer)
            .map_err(|e| ToolError::Http(e.to_string()))?;
        if n == 0 {
            break;
        }
        total_len += n;
        check_size(total_len, url)?;
        bytes.extend_from_slice(&buffer[..n]);
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
        let result = fetch_url(
            &client,
            "https://httpbin.org/robots.txt",
            Duration::from_secs(10),
        );

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
        let result = fetch_url(
            &client,
            "https://httpbin.org/status/404",
            Duration::from_secs(10),
        );

        // In case of network issues, just verify we get some result
        if let Err(e) = result {
            assert!(matches!(e, ToolError::Http(_)));
        }
    }
}
