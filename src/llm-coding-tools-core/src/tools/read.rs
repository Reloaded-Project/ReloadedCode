//! File reading operation.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::output::ToolOutput;
use crate::path::PathResolver;
use crate::tool_metadata::read as read_meta;
use crate::util::{truncate_line_with_ellipsis, ESTIMATED_CHARS_PER_LINE, TRUNCATION_ELLIPSIS};
use memchr::memchr;
use serde::Deserialize;
use serde_json::Value;
use std::borrow::Cow;
use std::fmt::Write;

/// Strips trailing CR from a line (for CRLF handling).
#[inline]
fn strip_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Processes a single line, appending it to output with optional line numbers.
#[inline]
fn process_line(
    line_bytes: &[u8],
    line_number: usize,
    output: &mut String,
    lines_output: &mut usize,
    max_line_length: usize,
    line_numbers: bool,
) {
    let line_bytes = strip_cr(line_bytes);
    let content: Cow<'_, str> = String::from_utf8_lossy(line_bytes);
    let (display_content, was_truncated) = truncate_line_with_ellipsis(&content, max_line_length);

    if *lines_output > 0 {
        output.push('\n');
    }

    if line_numbers {
        let _ = write!(output, "L{}: {}", line_number, display_content);
    } else {
        output.push_str(display_content);
    }

    if was_truncated {
        output.push_str(TRUNCATION_ELLIPSIS);
    }

    *lines_output += 1;
}

/// Serde-friendly read request owned by the core crate.
#[derive(Debug, Deserialize)]
pub struct ReadRequest {
    pub file_path: String,
    #[serde(default = "read_meta::default_offset")]
    pub offset: usize,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl ReadRequest {
    /// Parses a raw JSON tool payload into a read request.
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to read requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadSettings {
    default_limit: usize,
    max_limit: usize,
    max_line_length: usize,
    line_numbers: bool,
}

impl Default for ReadSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadSettings {
    /// Creates valid read settings with the standard defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            default_limit: read_meta::DEFAULT_LIMIT,
            max_limit: read_meta::DEFAULT_LIMIT,
            max_line_length: read_meta::MAX_LINE_LENGTH,
            line_numbers: true,
        }
    }

    /// Updates both read limits in one validated step.
    ///
    /// # Arguments
    /// - `default_limit`: Default line count when the request omits `limit`.
    /// - `max_limit`: Hard cap applied to any requested limit.
    ///
    /// # Returns
    /// - `Ok(Self)` with both limits updated.
    ///
    /// # Errors
    /// - Returns an error when either limit is below `MIN_LIMIT` or `default_limit > max_limit`.
    pub fn with_limits(mut self, default_limit: usize, max_limit: usize) -> ToolResult<Self> {
        ensure_read_limits(default_limit, max_limit)?;
        self.default_limit = default_limit;
        self.max_limit = max_limit;
        Ok(self)
    }

    /// Updates only the default limit while preserving the current max limit.
    pub fn with_default_limit(self, default_limit: usize) -> ToolResult<Self> {
        self.with_limits(default_limit, self.max_limit)
    }

    /// Updates only the max limit while preserving the current default limit.
    pub fn with_max_limit(self, max_limit: usize) -> ToolResult<Self> {
        self.with_limits(self.default_limit, max_limit)
    }

    /// Updates the per-line truncation length.
    pub fn with_max_line_length(mut self, max_line_length: usize) -> ToolResult<Self> {
        ensure_max_line_length(max_line_length)?;
        self.max_line_length = max_line_length;
        Ok(self)
    }

    /// Enables or disables line numbers in output.
    #[must_use]
    pub const fn with_line_numbers(mut self, line_numbers: bool) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Returns the default line limit.
    #[must_use]
    pub const fn default_limit(self) -> usize {
        self.default_limit
    }

    /// Returns the maximum line limit.
    #[must_use]
    pub const fn max_limit(self) -> usize {
        self.max_limit
    }

    /// Returns the maximum characters per line before truncation.
    #[must_use]
    pub const fn max_line_length(self) -> usize {
        self.max_line_length
    }

    /// Returns whether line numbers are included in output.
    #[must_use]
    pub const fn line_numbers(self) -> bool {
        self.line_numbers
    }
}

fn ensure_read_limits(default_limit: usize, max_limit: usize) -> ToolResult<()> {
    use crate::util::MIN_LIMIT;
    if default_limit < MIN_LIMIT {
        return Err(ToolError::validation_for(
            "default_limit",
            format!("default_limit must be >= {}", MIN_LIMIT),
        ));
    }
    if max_limit < MIN_LIMIT {
        return Err(ToolError::validation_for(
            "max_limit",
            format!("max_limit must be >= {}", MIN_LIMIT),
        ));
    }
    if default_limit > max_limit {
        return Err(ToolError::validation_for(
            "default_limit",
            format!("default_limit ({default_limit}) must be <= max_limit ({max_limit})"),
        ));
    }
    Ok(())
}

fn ensure_max_line_length(max_line_length: usize) -> ToolResult<()> {
    use crate::util::MIN_LINE_LENGTH;
    if max_line_length < MIN_LINE_LENGTH {
        return Err(ToolError::validation_for(
            "max_line_length",
            format!("max_line_length must be >= {}", MIN_LINE_LENGTH),
        ));
    }
    Ok(())
}

/// Reads a file and returns formatted content, optionally with line numbers.
///
/// When `line_numbers` is `true`, each line is prefixed with `L{number}: `.
/// When `false`, raw content is returned without prefixes.
#[maybe_async::maybe_async]
pub async fn read_file<R: PathResolver>(
    resolver: &R,
    request: ReadRequest,
    settings: ReadSettings,
) -> ToolResult<ToolOutput> {
    // Conditional trait import for consume() method
    #[cfg(feature = "blocking")]
    use std::io::BufRead as _;
    #[cfg(feature = "tokio")]
    use tokio::io::AsyncBufReadExt as _;

    let limit = request
        .limit
        .unwrap_or(settings.default_limit())
        .min(settings.max_limit());
    if limit == 0 {
        return Err(ToolError::validation_for("limit", "limit must be >= 1"));
    }

    let offset = request.offset;
    let max_line_length = settings.max_line_length();
    let line_numbers = settings.line_numbers();

    if offset == 0 {
        return Err(ToolError::OutOfBounds(
            "offset must be >= 1 (1-indexed)".into(),
        ));
    }

    let path = resolver.resolve(&request.file_path)?;
    let buf_capacity = (limit * ESTIMATED_CHARS_PER_LINE).next_power_of_two();
    let mut reader = fs::open_buffered(&path, buf_capacity).await?;

    let estimated_capacity = limit * ESTIMATED_CHARS_PER_LINE;
    let mut output = String::with_capacity(estimated_capacity);
    // Holds a partial line that spans multiple buffers.
    let mut overflow: Vec<u8> = Vec::new();
    let mut line_number = 0usize;
    let mut lines_output = 0usize;

    // Stream buffered chunks, splitting into lines as we go.
    loop {
        let buf = reader.fill_buf().await?;
        // Flush any trailing partial line at EOF.
        if buf.is_empty() {
            if !overflow.is_empty() {
                line_number += 1;
                if line_number >= offset && lines_output < limit {
                    process_line(
                        &overflow,
                        line_number,
                        &mut output,
                        &mut lines_output,
                        max_line_length,
                        line_numbers,
                    );
                }
            }
            break;
        }

        let mut pos = 0;
        while pos < buf.len() {
            // Fast newline search to delimit lines.
            if let Some(newline_offset) = memchr(b'\n', &buf[pos..]) {
                let newline_pos = pos + newline_offset;
                line_number += 1;

                // Only emit lines within the requested window.
                if line_number >= offset && lines_output < limit {
                    if overflow.is_empty() {
                        // Fast path: line is fully in this buffer.
                        process_line(
                            &buf[pos..newline_pos],
                            line_number,
                            &mut output,
                            &mut lines_output,
                            max_line_length,
                            line_numbers,
                        );
                    } else {
                        // Slow path: prepend buffered fragment.
                        overflow.extend_from_slice(&buf[pos..newline_pos]);
                        process_line(
                            &overflow,
                            line_number,
                            &mut output,
                            &mut lines_output,
                            max_line_length,
                            line_numbers,
                        );
                        overflow.clear();
                    }
                } else if !overflow.is_empty() {
                    overflow.clear();
                }

                pos = newline_pos + 1;

                if lines_output >= limit {
                    break;
                }
            } else {
                overflow.extend_from_slice(&buf[pos..]);
                pos = buf.len();
            }
        }

        reader.consume(pos);

        if lines_output >= limit {
            break;
        }
    }

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
    use crate::path::AbsolutePathResolver;
    use rstest::rstest;
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[maybe_async::maybe_async]
    async fn read_temp_file(
        content: &[u8],
        offset: usize,
        limit: usize,
        line_numbers: bool,
    ) -> ToolResult<ToolOutput> {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(content).unwrap();
        let resolver = AbsolutePathResolver;
        let settings = ReadSettings::new()
            .with_limits(limit, limit)
            .unwrap()
            .with_line_numbers(line_numbers);
        read_file::<_>(
            &resolver,
            ReadRequest {
                file_path: temp.path().to_str().unwrap().to_string(),
                offset,
                limit: Some(limit),
            },
            settings,
        )
        .await
    }

    // ReadSettings tests
    #[test]
    fn read_settings_should_create_standard_defaults() {
        let settings = ReadSettings::new();
        assert_eq!(settings.default_limit(), read_meta::DEFAULT_LIMIT);
        assert_eq!(settings.max_limit(), read_meta::DEFAULT_LIMIT);
        assert_eq!(settings.max_line_length(), read_meta::MAX_LINE_LENGTH);
        assert!(settings.line_numbers());
    }

    #[test]
    fn read_settings_should_accept_equal_minimum_limits() {
        let settings = ReadSettings::new().with_limits(1, 1).unwrap();
        assert_eq!(settings.default_limit(), 1);
        assert_eq!(settings.max_limit(), 1);
    }

    #[rstest]
    #[case::zero_default_limit(0, 1)]
    #[case::both_zero(0, 0)]
    #[case::default_limit_above_max(2, 1)]
    fn read_settings_should_reject_invalid_limit_pairs(
        #[case] default_limit: usize,
        #[case] max_limit: usize,
    ) {
        assert!(ReadSettings::new()
            .with_limits(default_limit, max_limit)
            .is_err());
    }

    #[test]
    fn read_settings_should_reject_zero_limits_from_individual_updates() {
        assert!(ReadSettings::new().with_default_limit(0).is_err());
        assert!(ReadSettings::new().with_max_limit(0).is_err());
    }

    #[test]
    fn read_settings_should_reject_max_limit_below_current_default() {
        let settings = ReadSettings::new().with_default_limit(100).unwrap();
        assert!(settings.with_max_limit(50).is_err());
    }

    #[test]
    fn read_settings_should_reject_short_max_line_length() {
        assert!(ReadSettings::new().with_max_line_length(3).is_err());
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn reads_basic_file_with_line_numbers() {
        let result = read_temp_file(b"hello\nworld\n", 1, 2000, true)
            .await
            .unwrap();
        assert_eq!(result.content, "L1: hello\nL2: world");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn reads_basic_file_without_line_numbers() {
        let result = read_temp_file(b"hello\nworld\n", 1, 2000, false)
            .await
            .unwrap();
        assert_eq!(result.content, "hello\nworld");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn errors_on_offset_zero() {
        let err = read_temp_file(b"test\n", 0, 10, true).await.unwrap_err();
        assert!(matches!(err, ToolError::OutOfBounds(_)));
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn truncates_long_line_with_ellipsis() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"abcdefghij\n").unwrap();
        let resolver = AbsolutePathResolver;

        let settings = ReadSettings::new()
            .with_limits(10, 10)
            .unwrap()
            .with_max_line_length(6)
            .unwrap()
            .with_line_numbers(false);

        let result = read_file::<_>(
            &resolver,
            ReadRequest {
                file_path: temp.path().to_str().unwrap().to_string(),
                offset: 1,
                limit: Some(10),
            },
            settings,
        )
        .await
        .unwrap();

        assert_eq!(result.content, "abc...");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_request_caps_requested_limit_at_max_limit() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"line1\nline2\nline3\n").unwrap();
        let resolver = AbsolutePathResolver;

        // Valid settings: default_limit (2) <= max_limit (3), but request limit (3)
        // will be capped at max_limit (3), then min with requested gives 3.
        // Actually, to test capping, we need max_limit to be lower than requested.
        // So: default_limit=1, max_limit=1, and request with limit=3 should cap at 1.
        let settings = ReadSettings::new()
            .with_limits(1, 1)
            .unwrap()
            .with_line_numbers(true);

        let result = read_file(
            &resolver,
            ReadRequest {
                file_path: temp.path().to_string_lossy().into_owned(),
                offset: 1,
                limit: Some(3),
            },
            settings,
        )
        .await
        .unwrap();

        assert_eq!(result.content, "L1: line1");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn read_request_rejects_zero_requested_limit() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"line1\n").unwrap();
        let resolver = AbsolutePathResolver;

        // Use valid settings, but request an explicit limit of 0
        let settings = ReadSettings::new()
            .with_limits(10, 10)
            .unwrap()
            .with_line_numbers(true);

        let err = read_file(
            &resolver,
            ReadRequest {
                file_path: temp.path().to_string_lossy().into_owned(),
                offset: 1,
                limit: Some(0), // Request explicitly asks for 0 lines
            },
            settings,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, ToolError::Validation { .. }));
    }
}
