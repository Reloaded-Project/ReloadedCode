//! Read file tool for reading file contents with optional line numbers.

use crate::error::{ToolError, ToolResult};
use crate::output::ToolOutput;
use crate::util::{truncate_line, validate_absolute_path, ESTIMATED_CHARS_PER_LINE};
use memchr::memchr;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use std::borrow::Cow;
use std::fmt::Write;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

const MAX_LINE_LENGTH: usize = 2000;
const DEFAULT_OFFSET: usize = 1;
const DEFAULT_LIMIT: usize = 2000;

fn default_offset() -> usize {
    DEFAULT_OFFSET
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// Arguments for the read file tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Absolute path to the file to read.
    pub file_path: String,
    /// 1-indexed line number to start reading from (default: 1).
    #[serde(default = "default_offset")]
    pub offset: usize,
    /// Maximum number of lines to return (default: 2000).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Tool for reading file contents with optional line numbers.
///
/// The const generic `LINE_NUMBERS` controls whether lines are prefixed
/// with `L{number}: `. When `true` (default), output includes line numbers
/// for easier editing. When `false`, raw content is returned.
///
/// # Examples
///
/// ```
/// use rig_coding_tools::tools::ReadTool;
///
/// // With line numbers (explicit type needed for inference)
/// let tool: ReadTool = ReadTool::new();
/// // or: ReadTool::<true>::new()
///
/// // Without line numbers
/// let raw_tool = ReadTool::<false>::new();
/// ```
#[derive(Debug, Clone, Default)]
pub struct ReadTool<const LINE_NUMBERS: bool = true>;

impl<const LINE_NUMBERS: bool> ReadTool<LINE_NUMBERS> {
    /// Creates a new read tool instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl<const LINE_NUMBERS: bool> Tool for ReadTool<LINE_NUMBERS> {
    const NAME: &'static str = "read";

    type Error = ToolError;
    type Args = ReadArgs;
    type Output = ToolOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let description = if LINE_NUMBERS {
            "Read file contents with line numbers. Returns lines prefixed with L{number}: format."
        } else {
            "Read file contents. Returns raw file content without line number prefixes."
        };
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: description.to_string(),
            parameters: serde_json::to_value(schema_for!(ReadArgs))
                .expect("schema serialization should never fail"),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        read_file::<LINE_NUMBERS>(&args.file_path, args.offset, args.limit).await
    }
}

/// Strips trailing CR from a line (for CRLF handling).
#[inline]
fn strip_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Processes a single line, appending it to output with optional line numbers.
///
/// This is the hot path - called for every line in the file within the requested range.
/// Uses zero-copy where possible via [`Cow`].
#[inline]
fn process_line<const LINE_NUMBERS: bool>(
    line_bytes: &[u8],
    line_number: usize,
    output: &mut String,
    lines_output: &mut usize,
) {
    // Strip trailing CR for CRLF line endings
    let line_bytes = strip_cr(line_bytes);

    // Convert to string with lossy UTF-8 handling (zero-copy for valid UTF-8)
    let content: Cow<'_, str> = String::from_utf8_lossy(line_bytes);

    // Truncate long lines
    let (truncated_content, _) = truncate_line(&content, MAX_LINE_LENGTH);

    // Add newline separator for subsequent lines
    // We do it here to avoid trailing newline at end of output
    if *lines_output > 0 {
        output.push('\n');
    }

    // Branch eliminated at compile time due to const generic
    if LINE_NUMBERS {
        // write! to String is infallible
        let _ = write!(output, "L{}: {}", line_number, truncated_content);
    } else {
        output.push_str(truncated_content);
    }

    *lines_output += 1;
}

/// Reads a file and returns formatted content, optionally with line numbers.
///
/// When `LINE_NUMBERS` is `true`, each line is prefixed with `L{number}: `.
/// When `false`, raw content is returned without prefixes.
async fn read_file<const LINE_NUMBERS: bool>(
    file_path: &str,
    offset: usize,
    limit: usize,
) -> ToolResult<ToolOutput> {
    // Validate arguments
    if offset == 0 {
        return Err(ToolError::OutOfBounds(
            "offset must be >= 1 (1-indexed)".into(),
        ));
    }
    if limit == 0 {
        return Err(ToolError::OutOfBounds("limit must be >= 1".into()));
    }

    let path = Path::new(file_path);
    validate_absolute_path(path)?;

    // Buffer for lines spanning multiple fills (rare case)
    // Rare enough for me to not alloc. Most files are under `limit * ESTIMATED_CHARS_PER_LINE`.
    let mut overflow: Vec<u8> = Vec::new();

    let file = File::open(path).await?;
    // Size buffer for expected content, rounded to next power of 2
    let buf_capacity = (limit * ESTIMATED_CHARS_PER_LINE).next_power_of_two();
    let mut reader = BufReader::with_capacity(buf_capacity, file);

    // Pre-allocate output based on expected content size
    let estimated_capacity = limit * ESTIMATED_CHARS_PER_LINE;
    let mut output = String::with_capacity(estimated_capacity);
    let mut line_number = 0usize;
    let mut lines_output = 0usize;

    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            // EOF - handle any remaining overflow as final line (no trailing newline)
            if !overflow.is_empty() {
                line_number += 1;
                if line_number >= offset && lines_output < limit {
                    process_line::<LINE_NUMBERS>(
                        &overflow,
                        line_number,
                        &mut output,
                        &mut lines_output,
                    );
                }
            }
            break;
        }

        let mut pos = 0;
        while pos < buf.len() {
            if let Some(newline_offset) = memchr(b'\n', &buf[pos..]) {
                let newline_pos = pos + newline_offset;
                line_number += 1;

                // Only process if within offset..offset+limit range
                if line_number >= offset && lines_output < limit {
                    if overflow.is_empty() {
                        // Common case: process directly from buffer (zero-copy)
                        process_line::<LINE_NUMBERS>(
                            &buf[pos..newline_pos],
                            line_number,
                            &mut output,
                            &mut lines_output,
                        );
                    } else {
                        // Rare case: complete the accumulated line
                        overflow.extend_from_slice(&buf[pos..newline_pos]);
                        process_line::<LINE_NUMBERS>(
                            &overflow,
                            line_number,
                            &mut output,
                            &mut lines_output,
                        );
                        overflow.clear();
                    }
                } else if !overflow.is_empty() {
                    // Line was being accumulated but we're skipping it
                    overflow.clear();
                }

                pos = newline_pos + 1;

                // Early exit if we've collected enough lines
                if lines_output >= limit {
                    break;
                }
            } else {
                // No newline found - line spans buffer boundary
                overflow.extend_from_slice(&buf[pos..]);
                pos = buf.len();
            }
        }

        reader.consume(pos);

        if lines_output >= limit {
            break;
        }
    }

    // Check if offset exceeded file length
    if line_number < offset {
        return Err(ToolError::OutOfBounds(format!(
            "offset {} exceeds file length of {} lines",
            offset, line_number
        )));
    }

    Ok(ToolOutput::new(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    async fn read_temp_file<const LINE_NUMBERS: bool>(
        content: &[u8],
        offset: usize,
        limit: usize,
    ) -> ToolResult<ToolOutput> {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(content).unwrap();
        read_file::<LINE_NUMBERS>(temp.path().to_str().unwrap(), offset, limit).await
    }

    #[tokio::test]
    async fn reads_basic_file() {
        let result = read_temp_file::<true>(b"hello\nworld\n", 1, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "L1: hello\nL2: world");
    }

    #[tokio::test]
    async fn reads_basic_file_no_line_numbers() {
        let result = read_temp_file::<false>(b"hello\nworld\n", 1, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "hello\nworld");
    }

    #[tokio::test]
    async fn reads_with_offset() {
        let result = read_temp_file::<true>(b"one\ntwo\nthree\n", 2, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "L2: two\nL3: three");
    }

    #[tokio::test]
    async fn reads_with_offset_no_line_numbers() {
        let result = read_temp_file::<false>(b"one\ntwo\nthree\n", 2, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "two\nthree");
    }

    #[tokio::test]
    async fn reads_with_limit() {
        let result = read_temp_file::<true>(b"one\ntwo\nthree\n", 1, 2)
            .await
            .unwrap();
        assert_eq!(result.content, "L1: one\nL2: two");
    }

    #[tokio::test]
    async fn reads_with_offset_and_limit() {
        let result = read_temp_file::<true>(b"one\ntwo\nthree\nfour\n", 2, 2)
            .await
            .unwrap();
        assert_eq!(result.content, "L2: two\nL3: three");
    }

    #[tokio::test]
    async fn handles_crlf_line_endings() {
        let result = read_temp_file::<true>(b"line1\r\nline2\r\n", 1, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "L1: line1\nL2: line2");
    }

    #[tokio::test]
    async fn handles_non_utf8_content() {
        let result = read_temp_file::<true>(b"\xff\xfe\nplain\n", 1, 2000)
            .await
            .unwrap();
        assert!(result.content.contains("L1:"));
        assert!(result.content.contains('\u{FFFD}')); // replacement char
        assert!(result.content.contains("L2: plain"));
    }

    #[tokio::test]
    async fn truncates_long_lines() {
        let long_line = "x".repeat(MAX_LINE_LENGTH + 100);
        let content = format!("{}\n", long_line);
        let result = read_temp_file::<true>(content.as_bytes(), 1, 1)
            .await
            .unwrap();
        let expected = format!("L1: {}", "x".repeat(MAX_LINE_LENGTH));
        assert_eq!(result.content, expected);
    }

    #[tokio::test]
    async fn truncates_long_lines_no_line_numbers() {
        let long_line = "x".repeat(MAX_LINE_LENGTH + 100);
        let content = format!("{}\n", long_line);
        let result = read_temp_file::<false>(content.as_bytes(), 1, 1)
            .await
            .unwrap();
        assert_eq!(result.content, "x".repeat(MAX_LINE_LENGTH));
    }

    #[tokio::test]
    async fn errors_on_offset_zero() {
        let err = read_temp_file::<true>(b"test\n", 0, 10).await.unwrap_err();
        assert!(matches!(err, ToolError::OutOfBounds(_)));
    }

    #[tokio::test]
    async fn errors_on_limit_zero() {
        let err = read_temp_file::<true>(b"test\n", 1, 0).await.unwrap_err();
        assert!(matches!(err, ToolError::OutOfBounds(_)));
    }

    #[tokio::test]
    async fn errors_on_offset_exceeds_file() {
        let err = read_temp_file::<true>(b"one\ntwo\n", 10, 100)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::OutOfBounds(_)));
    }

    #[tokio::test]
    async fn errors_on_relative_path() {
        let err = read_file::<true>("relative/path.txt", 1, 100)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidPath(_)));
    }

    #[tokio::test]
    async fn errors_on_nonexistent_file() {
        let err = read_file::<true>("/nonexistent/file.txt", 1, 100)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
    }

    #[tokio::test]
    async fn handles_empty_file() {
        let result = read_temp_file::<true>(b"", 1, 100).await;
        // Empty file with offset 1 should error
        assert!(matches!(result, Err(ToolError::OutOfBounds(_))));
    }

    #[tokio::test]
    async fn handles_file_without_trailing_newline() {
        let result = read_temp_file::<true>(b"no trailing newline", 1, 100)
            .await
            .unwrap();
        assert_eq!(result.content, "L1: no trailing newline");
    }

    #[tokio::test]
    async fn handles_file_without_trailing_newline_no_line_numbers() {
        let result = read_temp_file::<false>(b"no trailing newline", 1, 100)
            .await
            .unwrap();
        assert_eq!(result.content, "no trailing newline");
    }
}
