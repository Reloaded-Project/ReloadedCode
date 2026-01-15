//! Web content fetching operation.

use crate::error::{ToolError, ToolResult};
use html_to_markdown_rs::{convert, ConversionOptions, PreprocessingOptions, PreprocessingPreset};

/// Maximum response size to accept (5MB).
pub(crate) const MAX_RESPONSE_SIZE: usize = 5 * 1_024 * 1_024;

/// Result from URL fetch operation.
#[derive(Debug, Clone)]
pub struct WebFetchOutput {
    /// The processed content (HTML converted to markdown, JSON prettified).
    pub content: String,
    /// The Content-Type header value.
    pub content_type: String,
    /// Original byte length before processing.
    pub byte_length: usize,
}

/// Processes raw response content based on content type.
pub(crate) fn process_content(raw_content: &str, content_type: &str) -> String {
    if content_type.contains("text/html") {
        html_to_markdown(raw_content)
    } else if content_type.contains("application/json") {
        format_json(raw_content)
    } else {
        raw_content.to_owned()
    }
}

/// Categorizes reqwest errors into appropriate [`ToolError`] variants.
pub(crate) fn categorize_reqwest_error(e: reqwest::Error, url: &str) -> ToolError {
    if e.is_timeout() {
        ToolError::Timeout(format!("Request timed out for {}", url))
    } else if e.is_connect() {
        ToolError::Http(format!("Connection failed for {}: {}", url, e))
    } else if e.is_redirect() {
        ToolError::Http(format!("Too many redirects for {}", url))
    } else {
        ToolError::Http(e.to_string())
    }
}

/// Returns an error if the response size exceeds the maximum.
#[inline]
pub(crate) fn check_size(len: usize, url: &str) -> ToolResult<()> {
    if len > MAX_RESPONSE_SIZE {
        return Err(ToolError::Http(format!(
            "Response too large: {} bytes (max {}) for {}",
            len, MAX_RESPONSE_SIZE, url
        )));
    }
    Ok(())
}

/// Converts HTML to markdown for LLM-friendly output.
pub fn html_to_markdown(html: &str) -> String {
    let options = ConversionOptions {
        preprocessing: PreprocessingOptions {
            enabled: true,
            preset: PreprocessingPreset::Aggressive,
            remove_navigation: true,
            remove_forms: true,
        },
        strip_tags: vec![
            "img".into(),
            "svg".into(),
            "script".into(),
            "style".into(),
            "noscript".into(),
        ],
        ..Default::default()
    };

    convert(html, Some(options)).unwrap_or_else(|_| html.to_string())
}

/// Formats JSON content for readability.
pub fn format_json(json_str: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| json_str.to_string()),
        Err(_) => json_str.to_string(),
    }
}

#[cfg(not(feature = "blocking"))]
mod async_impl;
#[cfg(not(feature = "blocking"))]
pub use async_impl::fetch_url;

#[cfg(feature = "blocking")]
mod blocking_impl;
#[cfg(feature = "blocking")]
pub use blocking_impl::fetch_url;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_markdown_strips_scripts() {
        let html = "<p>Before</p><script>alert('xss')</script><p>After</p>";
        let result = html_to_markdown(html);
        assert!(!result.contains("alert"));
    }

    #[test]
    fn format_json_prettifies() {
        let json = r#"{"a":1}"#;
        let result = format_json(json);
        assert!(result.contains("\"a\": 1"));
    }

    #[test]
    fn format_json_returns_original_on_invalid() {
        let invalid = "not json";
        assert_eq!(format_json(invalid), "not json");
    }

    #[test]
    fn check_size_ok_for_small_content() {
        assert!(check_size(1000, "http://example.com").is_ok());
    }

    #[test]
    fn check_size_fails_for_large_content() {
        assert!(check_size(MAX_RESPONSE_SIZE + 1, "http://example.com").is_err());
    }
}
