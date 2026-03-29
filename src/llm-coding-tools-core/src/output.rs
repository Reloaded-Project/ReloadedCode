//! Common output types for tool responses.

#[cfg(any(feature = "tokio", feature = "blocking"))]
use crate::tools::WebFetchOutput;
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

#[cfg(any(feature = "tokio", feature = "blocking"))]
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
    use rstest::rstest;

    /// Verifies that ToolOutput constructors correctly set the truncated flag.
    #[rstest]
    #[case::new_creates_non_truncated(false, "content")]
    #[case::truncated_marks_truncated(true, "partial")]
    fn tool_output_creation(#[case] is_truncated: bool, #[case] content: &str) {
        let output = if is_truncated {
            ToolOutput::truncated(content)
        } else {
            ToolOutput::new(content)
        };
        assert_eq!(output.content, content);
        assert_eq!(output.truncated, is_truncated);
    }

    /// Verifies that the truncated field is only serialized when true.
    /// ToolOutput uses `#[serde(skip_serializing_if)]` to omit the field
    /// when false, producing cleaner JSON output.
    ///
    /// We verify this behaviour specifically to ensure the LLM does not receive
    /// unnecessary tokens for default values that provide no information.
    #[rstest]
    #[case::without_truncated_when_false(false)]
    #[case::with_truncated_when_true(true)]
    fn tool_output_serialization(#[case] truncated: bool) {
        let output = if truncated {
            ToolOutput::truncated("content")
        } else {
            ToolOutput::new("content")
        };
        let json = serde_json::to_string(&output).unwrap();
        assert_eq!(json.contains("truncated"), truncated);
    }

    #[cfg(any(feature = "tokio", feature = "blocking"))]
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
