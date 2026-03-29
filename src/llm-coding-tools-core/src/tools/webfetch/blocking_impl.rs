//! Blocking web content fetching.

use super::{categorize_reqwest_error, check_size, process_content, WebFetchOutput};
use crate::error::{ToolError, ToolResult};
use std::io::{BufRead, BufReader};
use std::time::Duration;

/// Fetches content from a URL and returns processed content.
///
/// - HTML is converted to markdown
/// - JSON is pretty-printed
/// - Other content types returned as-is
/// - Response size is limited to `max_response_size` bytes
///
/// # Arguments
///
/// * `client` - The HTTP client to use
/// * `url` - The URL to fetch
/// * `timeout_ms` - Timeout in milliseconds (must be >= 1 and <= max_timeout_ms)
/// * `max_timeout_ms` - Maximum allowed timeout in milliseconds
/// * `max_response_size` - Maximum response size in bytes
///
/// # Errors
///
/// Returns `ToolError::Validation` if timeout_ms is 0 or exceeds max_timeout_ms.
pub fn fetch_url(
    client: &reqwest::blocking::Client,
    url: &str,
    timeout_ms: u32,
    max_timeout_ms: u32,
    max_response_size: usize,
) -> ToolResult<WebFetchOutput> {
    if timeout_ms == 0 {
        return Err(ToolError::Validation(
            "timeout_ms must be at least 1".to_string(),
        ));
    }
    if timeout_ms > max_timeout_ms {
        return Err(ToolError::Validation(format!(
            "timeout_ms exceeds maximum allowed value of {}",
            max_timeout_ms
        )));
    }

    let timeout = Duration::from_millis(timeout_ms as u64);
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
        check_size(len, url, max_response_size)?;
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
        check_size(total_len, url, max_response_size)?;

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
    use rstest::rstest;
    use std::sync::mpsc;
    use std::sync::Arc;
    use tokio::runtime::Runtime;
    use tokio::sync::Notify;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .build()
            .expect("client build failed")
    }

    /// Handle to a running mock server. When dropped, signals the server to shut down
    /// and waits for the server thread to exit, ensuring proper cleanup.
    struct MockServerHandle {
        uri: String,
        shutdown: Arc<Notify>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl MockServerHandle {
        fn uri(&self) -> &str {
            &self.uri
        }
    }

    impl Drop for MockServerHandle {
        fn drop(&mut self) {
            self.shutdown.notify_one();
            if let Some(thread) = self.thread.take() {
                thread.join().ok();
            }
        }
    }

    /// Starts a wiremock server in a dedicated tokio thread for blocking tests.
    /// Returns a handle that shuts down the server and joins the thread when dropped.
    fn start_mock_server(
        setup: impl FnOnce(MockServer) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
            + Send
            + 'static,
    ) -> MockServerHandle {
        let (tx, rx) = mpsc::channel::<String>();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let thread = std::thread::spawn(move || {
            let rt = Runtime::new().expect("Failed to create tokio runtime");
            rt.block_on(async {
                let server = MockServer::start().await;
                let uri = server.uri();
                tx.send(uri).expect("Failed to send server URI");
                setup(server).await;
                // Wait for shutdown signal
                shutdown_clone.notified().await;
            })
        });

        let uri = rx.recv().expect("Failed to receive server URI");
        MockServerHandle {
            uri,
            shutdown,
            thread: Some(thread),
        }
    }

    /// Verifies that invalid timeout values are rejected before making any
    /// network request. The URL is intentionally unreachable.
    #[rstest]
    #[case::zero_timeout(0, 10_000)]
    #[case::exceeds_max(11_000, 10_000)]
    fn rejects_invalid_timeout(#[case] timeout_ms: u32, #[case] max_timeout_ms: u32) {
        let client = test_client();

        let result = fetch_url(
            &client,
            "http://localhost:1",
            timeout_ms,
            max_timeout_ms,
            5 * 1024 * 1024,
        );

        assert!(matches!(result, Err(ToolError::Validation(_))));
    }

    #[test]
    fn fetches_plain_text() {
        let server = start_mock_server(|mock_server| {
            Box::pin(async move {
                Mock::given(method("GET"))
                    .and(path("/robots.txt"))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .set_body_bytes("User-agent: *\nDisallow: /")
                            .insert_header("content-type", "text/plain"),
                    )
                    .mount(&mock_server)
                    .await;
            })
        });

        let client = test_client();
        let result = fetch_url(
            &client,
            &format!("{}/robots.txt", server.uri()),
            10_000,
            20_000,
            5 * 1024 * 1024,
        );

        let output = result.expect("Should successfully fetch");
        assert!(output.content.contains("User-agent"));
        assert!(output.content_type.contains("text/plain"));
    }

    #[test]
    fn handles_404() {
        let server = start_mock_server(|mock_server| {
            Box::pin(async move {
                Mock::given(method("GET"))
                    .and(path("/not-found"))
                    .respond_with(ResponseTemplate::new(404))
                    .mount(&mock_server)
                    .await;
            })
        });

        let client = test_client();
        let result = fetch_url(
            &client,
            &format!("{}/not-found", server.uri()),
            10_000,
            20_000,
            5 * 1024 * 1024,
        );

        assert!(
            matches!(result, Err(ToolError::Http(_))),
            "Should return HTTP error for 404"
        );
    }
}
