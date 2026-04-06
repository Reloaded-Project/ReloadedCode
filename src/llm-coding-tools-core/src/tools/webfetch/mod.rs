//! Web content fetching operation.

use crate::error::{ToolError, ToolResult};
use crate::tool_metadata::webfetch as webfetch_meta;
use crate::util::MIN_TIMEOUT_MS;
use html_to_markdown_rs::{
    convert, ConversionOptions, ConversionResult, PreprocessingOptions, PreprocessingPreset,
};
use serde::Deserialize;
use serde_json::Value;

/// Serde-friendly webfetch request owned by the core crate.
#[derive(Debug, Clone, Deserialize)]
pub struct WebFetchRequest {
    /// The URL to fetch.
    pub url: String,
    /// Timeout in milliseconds. If omitted, uses the tool's default timeout.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

impl WebFetchRequest {
    /// Parses a raw JSON tool payload into a webfetch request.
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to webfetch requests.
///
/// Controls request duration using a two-timeout model:
/// - **Default timeout**: applied when the request doesn't specify one.
/// - **Max timeout**: hard cap applied regardless of what the caller requests.
///
/// A separate `max_response_size` field limits downloaded bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebFetchSettings {
    default_timeout_ms: u32,
    max_timeout_ms: u32,
    max_response_size: usize,
}

impl Default for WebFetchSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl WebFetchSettings {
    /// Creates valid webfetch settings with the standard timeout and size limits.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            default_timeout_ms: webfetch_meta::DEFAULT_TIMEOUT_MS,
            max_timeout_ms: webfetch_meta::MAX_TIMEOUT_MS,
            max_response_size: webfetch_meta::MAX_RESPONSE_SIZE,
        }
    }

    /// Sets the default and maximum request timeout in one validated step.
    ///
    /// The default timeout is used when the request doesn't specify one; the
    /// max timeout caps any explicitly requested duration. Both must be at
    /// least [`MIN_TIMEOUT_MS`], and `default_timeout_ms` must not exceed
    /// `max_timeout_ms`.
    ///
    /// # Arguments
    /// - `default_timeout_ms`: Timeout used when the request omits `timeout_ms`.
    /// - `max_timeout_ms`: Upper bound on request duration regardless of the request.
    ///
    /// # Errors
    /// - Returns an error when either timeout is below [`MIN_TIMEOUT_MS`] or
    ///   `default_timeout_ms > max_timeout_ms`.
    ///
    /// [`MIN_TIMEOUT_MS`]: crate::util::MIN_TIMEOUT_MS
    pub fn with_timeouts(
        mut self,
        default_timeout_ms: u32,
        max_timeout_ms: u32,
    ) -> ToolResult<Self> {
        ensure_timeouts(default_timeout_ms, max_timeout_ms)?;
        self.default_timeout_ms = default_timeout_ms;
        self.max_timeout_ms = max_timeout_ms;
        Ok(self)
    }

    /// Sets the timeout used when the request doesn't specify one.
    ///
    /// The new value must not exceed the current max timeout.
    ///
    /// # Errors
    /// - Returns an error when `default_timeout_ms` is below
    ///   [`MIN_TIMEOUT_MS`] or exceeds the current max timeout.
    ///
    /// [`MIN_TIMEOUT_MS`]: crate::util::MIN_TIMEOUT_MS
    pub fn with_default_timeout_ms(self, default_timeout_ms: u32) -> ToolResult<Self> {
        self.with_timeouts(default_timeout_ms, self.max_timeout_ms)
    }

    /// Sets the hard cap on request duration regardless of what the caller
    /// specifies.
    ///
    /// The new value must be at least as large as the current default timeout.
    ///
    /// # Errors
    /// - Returns an error when `max_timeout_ms` is below
    ///   [`MIN_TIMEOUT_MS`] or below the current default timeout.
    ///
    /// [`MIN_TIMEOUT_MS`]: crate::util::MIN_TIMEOUT_MS
    pub fn with_max_timeout_ms(self, max_timeout_ms: u32) -> ToolResult<Self> {
        self.with_timeouts(self.default_timeout_ms, max_timeout_ms)
    }

    /// Updates the maximum response size in bytes.
    ///
    /// # Errors
    /// - Returns an error when `max_response_size` is below [`MIN_LIMIT`].
    ///
    /// [`MIN_LIMIT`]: crate::util::MIN_LIMIT
    pub fn with_max_response_size(mut self, max_response_size: usize) -> ToolResult<Self> {
        use crate::util::MIN_LIMIT;
        if max_response_size < MIN_LIMIT {
            return Err(ToolError::validation_for(
                "max_response_size",
                format!("max_response_size must be >= {}", MIN_LIMIT),
            ));
        }
        self.max_response_size = max_response_size;
        Ok(self)
    }

    /// Returns the timeout used when the request doesn't specify one.
    #[must_use]
    pub const fn default_timeout_ms(self) -> u32 {
        self.default_timeout_ms
    }

    /// Returns the hard cap on request duration regardless of the request.
    #[must_use]
    pub const fn max_timeout_ms(self) -> u32 {
        self.max_timeout_ms
    }

    /// Returns the maximum response size in bytes.
    #[must_use]
    pub const fn max_response_size(self) -> usize {
        self.max_response_size
    }
}

fn ensure_timeouts(default_timeout_ms: u32, max_timeout_ms: u32) -> ToolResult<()> {
    if default_timeout_ms < MIN_TIMEOUT_MS {
        return Err(ToolError::validation_for(
            "default_timeout_ms",
            format!("default_timeout_ms must be >= {}", MIN_TIMEOUT_MS),
        ));
    }
    if max_timeout_ms < MIN_TIMEOUT_MS {
        return Err(ToolError::validation_for(
            "max_timeout_ms",
            format!("max_timeout_ms must be >= {}", MIN_TIMEOUT_MS),
        ));
    }
    if default_timeout_ms > max_timeout_ms {
        return Err(ToolError::validation_for(
            "default_timeout_ms",
            format!(
                "default_timeout_ms ({default_timeout_ms}) must be <= max_timeout_ms ({max_timeout_ms})"
            ),
        ));
    }
    Ok(())
}

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

/// Categorises reqwest errors into appropriate [`ToolError`] variants.
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
pub(crate) fn check_size(len: usize, url: &str, max_size: usize) -> ToolResult<()> {
    if len > max_size {
        return Err(ToolError::Http(format!(
            "Response too large: {} bytes (max {}) for {}",
            len, max_size, url
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

    convert(html, Some(options))
        .ok()
        .and_then(|result: ConversionResult| result.content)
        .unwrap_or_else(|| html.to_string())
}

/// Formats JSON content for readability.
pub fn format_json(json_str: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| json_str.to_string()),
        Err(_) => json_str.to_string(),
    }
}

#[cfg(feature = "tokio")]
mod tokio_impl;
#[cfg(feature = "tokio")]
pub use tokio_impl::fetch_url;

#[cfg(all(feature = "blocking", not(feature = "tokio")))]
mod blocking_impl;
#[cfg(all(feature = "blocking", not(feature = "tokio")))]
pub use blocking_impl::fetch_url;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::MIN_TIMEOUT_MS;

    // WebFetchSettings tests
    #[test]
    fn webfetch_settings_should_create_standard_defaults() {
        let settings = WebFetchSettings::new();
        assert_eq!(
            settings.default_timeout_ms(),
            webfetch_meta::DEFAULT_TIMEOUT_MS
        );
        assert_eq!(settings.max_timeout_ms(), webfetch_meta::MAX_TIMEOUT_MS);
        assert_eq!(
            settings.max_response_size(),
            webfetch_meta::MAX_RESPONSE_SIZE
        );
    }

    #[test]
    fn webfetch_settings_should_reject_timeout_below_minimum() {
        let below_min = MIN_TIMEOUT_MS - 1;
        assert!(WebFetchSettings::new()
            .with_default_timeout_ms(below_min)
            .is_err());
        assert!(WebFetchSettings::new()
            .with_max_timeout_ms(below_min)
            .is_err());
    }

    #[test]
    fn webfetch_settings_should_reject_default_timeout_above_max_timeout() {
        assert!(WebFetchSettings::new()
            .with_timeouts(30_001, 30_000)
            .is_err());
    }

    #[test]
    fn webfetch_settings_should_reject_zero_max_response_size() {
        assert!(WebFetchSettings::new().with_max_response_size(0).is_err());
    }

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
        assert!(check_size(1000, "http://example.com", 5 * 1024 * 1024).is_ok());
    }

    #[test]
    fn check_size_fails_for_large_content() {
        let max_size = 5 * 1024 * 1024;
        assert!(check_size(max_size + 1, "http://example.com", max_size).is_err());
    }
}
