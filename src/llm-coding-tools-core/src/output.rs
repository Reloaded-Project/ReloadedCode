//! Common output types for tool responses.

use crate::operations::WebFetchOutput;
use serde::Serialize;

/// Wrapper for tool output with truncation metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ToolOutput {
    /// The main content returned by the tool.
    pub content: String,
    /// Whether the output was truncated due to size limits.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

impl ToolOutput {
    /// Creates a new output with the given content.
    #[inline]
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            truncated: false,
        }
    }

    /// Creates a truncated output.
    #[inline]
    pub fn truncated(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            truncated: true,
        }
    }
}

impl From<String> for ToolOutput {
    fn from(content: String) -> Self {
        Self::new(content)
    }
}

impl From<&str> for ToolOutput {
    fn from(content: &str) -> Self {
        Self::new(content)
    }
}

impl From<WebFetchOutput> for ToolOutput {
    fn from(output: WebFetchOutput) -> Self {
        Self::new(format!(
            "[{} - {} bytes]\n\n{}",
            output.content_type, output.byte_length, output.content
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_output_new_creates_non_truncated() {
        let output = ToolOutput::new("content");
        assert_eq!(output.content, "content");
        assert!(!output.truncated);
    }

    #[test]
    fn tool_output_truncated_marks_truncated() {
        let output = ToolOutput::truncated("partial");
        assert!(output.truncated);
    }

    #[test]
    fn tool_output_from_string() {
        let output: ToolOutput = "hello".into();
        assert_eq!(output.content, "hello");
    }

    #[test]
    fn tool_output_serializes_without_truncated_when_false() {
        let output = ToolOutput::new("content");
        let json = serde_json::to_string(&output).unwrap();
        assert!(!json.contains("truncated"));
    }

    #[test]
    fn tool_output_serializes_with_truncated_when_true() {
        let output = ToolOutput::truncated("content");
        let json = serde_json::to_string(&output).unwrap();
        assert!(json.contains("truncated"));
    }

    #[test]
    fn tool_output_from_webfetch_output() {
        let webfetch = WebFetchOutput {
            content: "Hello, world!".to_string(),
            content_type: "text/plain".to_string(),
            byte_length: 13,
        };
        let output: ToolOutput = webfetch.into();
        assert_eq!(output.content, "[text/plain - 13 bytes]\n\nHello, world!");
        assert!(!output.truncated);
    }
}
