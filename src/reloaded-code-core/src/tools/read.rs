//! File reading operation.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::output::ToolOutput;
use crate::path::PathResolver;
use crate::tool_metadata::read as read_meta;
use crate::util::{
    push_padded_usize, truncate_line_with_ellipsis, ESTIMATED_CHARS_PER_LINE, TRUNCATION_ELLIPSIS,
};
use memchr::{memchr, memchr_iter};
use serde::Deserialize;
use serde_json::Value;

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
    ///
    /// # Errors
    /// - Returns [`ToolError::Json`] when the JSON payload cannot be deserialized
    ///   into a [`ReadRequest`] (e.g., missing required `file_path` field or
    ///   invalid field types).
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to read requests.
///
/// Controls how many lines a read returns using a two-limit model:
/// - **Default limit**: line count used when the caller doesn't specify one.
/// - **Max limit**: hard cap applied regardless of what the caller requests.
///
/// Additional settings control per-line truncation and line-number display.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn new() -> Self {
        Self {
            default_limit: read_meta::DEFAULT_LIMIT,
            max_limit: read_meta::DEFAULT_LIMIT,
            max_line_length: read_meta::MAX_LINE_LENGTH,
            line_numbers: true,
        }
    }

    /// Sets the line count used when the caller doesn't specify one.
    ///
    /// The new value must not exceed the current max limit.
    ///
    /// # Errors
    /// - Returns an error when `default_limit` is below [`MIN_LIMIT`] or
    ///   exceeds the current max limit.
    ///
    /// [`MIN_LIMIT`]: crate::util::MIN_LIMIT
    pub fn with_default_limit(self, default_limit: usize) -> ToolResult<Self> {
        let max_limit = self.max_limit;
        self.with_limits(default_limit, max_limit)
    }

    /// Sets the hard cap on lines returned regardless of what the caller
    /// requests.
    ///
    /// The new value must be at least as large as the current default limit.
    ///
    /// # Errors
    /// - Returns an error when `max_limit` is below [`MIN_LIMIT`] or below
    ///   the current default limit.
    ///
    /// [`MIN_LIMIT`]: crate::util::MIN_LIMIT
    pub fn with_max_limit(self, max_limit: usize) -> ToolResult<Self> {
        let default_limit = self.default_limit;
        self.with_limits(default_limit, max_limit)
    }

    /// Sets the default and maximum line count for read operations in one
    /// validated step.
    ///
    /// The default limit is used when the caller doesn't specify a line count;
    /// the max limit caps any explicitly requested count. Both limits must be
    /// at least [`MIN_LIMIT`], and `default_limit` must not exceed `max_limit`.
    ///
    /// # Arguments
    /// - `default_limit`: Line count used when the request omits `limit`.
    /// - `max_limit`: Upper bound on lines returned regardless of the request.
    ///
    /// # Errors
    /// - Returns an error when either limit is below [`MIN_LIMIT`] or
    ///   `default_limit > max_limit`.
    ///
    /// [`MIN_LIMIT`]: crate::util::MIN_LIMIT
    pub fn with_limits(mut self, default_limit: usize, max_limit: usize) -> ToolResult<Self> {
        ensure_read_limits(default_limit, max_limit)?;
        self.default_limit = default_limit;
        self.max_limit = max_limit;
        Ok(self)
    }

    /// Updates the per-line truncation length.
    ///
    /// # Errors
    /// - Returns an error when `max_line_length` is below
    ///   [`MIN_LINE_LENGTH`].
    ///
    /// [`MIN_LINE_LENGTH`]: crate::util::MIN_LINE_LENGTH
    pub fn with_max_line_length(mut self, max_line_length: usize) -> ToolResult<Self> {
        ensure_max_line_length(max_line_length)?;
        self.max_line_length = max_line_length;
        Ok(self)
    }

    /// Enables or disables line numbers in output.
    ///
    /// # Arguments
    /// - `line_numbers` - `true` to prefix each line with its line number.
    ///
    /// # Returns
    /// - The modified [`ReadSettings`] with the updated flag.
    #[must_use]
    pub fn with_line_numbers(mut self, line_numbers: bool) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Returns the line count used when the caller doesn't specify one.
    ///
    /// # Returns
    /// - The configured default line limit.
    #[must_use]
    pub const fn default_limit(&self) -> usize {
        self.default_limit
    }

    /// Returns the hard cap on lines returned regardless of the request.
    ///
    /// # Returns
    /// - The configured maximum line limit.
    #[must_use]
    pub const fn max_limit(&self) -> usize {
        self.max_limit
    }

    /// Returns the maximum characters per line before truncation.
    ///
    /// # Returns
    /// - The configured per-line truncation length.
    #[must_use]
    pub const fn max_line_length(&self) -> usize {
        self.max_line_length
    }

    /// Returns whether line numbers are included in output.
    ///
    /// # Returns
    /// - `true` when line numbers are enabled.
    #[must_use]
    pub const fn line_numbers(&self) -> bool {
        self.line_numbers
    }
}

#[cfg(feature = "blocking")]
type BufFile = std::io::BufReader<std::fs::File>;
#[cfg(feature = "tokio")]
type BufFile = tokio::io::BufReader<tokio::fs::File>;

/// Reads a range of lines from a file using buffered, streaming I/O with
/// SIMD-accelerated newline scanning.
///
/// The function opens the file at the resolved path, skips to the requested
/// 1-indexed `offset`, then streams lines into an output string. Each line
/// can optionally carry a `{number}: ` prefix and is truncated to
/// `max_line_length` when necessary.
///
/// # Arguments
/// - `resolver`: [`PathResolver`] used to resolve `request.file_path` to a filesystem path.
/// - `request`: [`ReadRequest`] carrying the file path, 1-indexed offset, and optional line limit.
/// - `settings`: [`ReadSettings`] controlling line numbers and max line length.
///
/// # Returns
/// - [`ToolOutput`] containing the requested line range, each line optionally
///   prefixed with a line number and truncated to `max_line_length`.
///
/// # Errors
/// - Returns [`ToolError::OutOfBounds`] when `offset` is `0` or exceeds the file's line count.
/// - Returns [`ToolError::validation_for`] when `limit` resolves to `0`.
/// - Returns an I/O error when the file cannot be opened or read.
#[maybe_async::maybe_async]
pub async fn read_file<R: PathResolver>(
    resolver: &R,
    request: ReadRequest,
    settings: &ReadSettings,
) -> ToolResult<ToolOutput> {
    // Conditional trait import for consume() method
    #[cfg(feature = "blocking")]
    use std::io::BufRead as _;
    #[cfg(feature = "tokio")]
    use tokio::io::AsyncBufReadExt as _;

    // Resolve the effective line limit: fall back to the configured default,
    // then clamp to the hard max so callers cannot exceed it.
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

    // Reject offset 0 early - the API is 1-indexed.
    if offset == 0 {
        return Err(ToolError::OutOfBounds(
            "offset must be >= 1 (1-indexed)".into(),
        ));
    }

    // Resolve the logical path to a filesystem path.
    let path = resolver.resolve(&request.file_path)?;

    // Open the file with a buffered reader sized proportionally to the
    // expected output, capped at 1 MiB to avoid over-allocating on huge limits.
    let buf_capacity = limit
        .saturating_mul(ESTIMATED_CHARS_PER_LINE)
        .min(1_048_576);
    let mut reader = fs::open_buffered(&path, buf_capacity).await?;

    // Compute the width of the line number and "{number}: " prefix so the
    // output buffer can be pre-sized accurately. Derives digit count from
    // the last line number.
    let line_number_width = if line_numbers {
        let last_line = offset.saturating_add(limit).saturating_sub(1);
        last_line.checked_ilog10().unwrap_or(0) as usize + 1
    } else {
        0
    };
    let line_prefix_len = line_number_width + 2; // ": "
    let estimated_capacity = limit.saturating_mul(ESTIMATED_CHARS_PER_LINE + line_prefix_len);
    let mut output = String::with_capacity(estimated_capacity);
    // Holds a partial line that spans multiple buffered chunks.
    let mut overflow: Vec<u8> = Vec::new();
    let mut line_number = 0usize;
    let mut lines_output = 0usize;

    if offset > 1 {
        line_number = skip_to_line(&mut reader, offset - 1).await?;
    }

    // Stream buffered chunks, extracting and emitting lines within the
    // [offset, offset+limit) window until the limit is satisfied or EOF.
    loop {
        let buf = reader.fill_buf().await?;
        // Flush any trailing partial line at EOF.
        if buf.is_empty() {
            if !overflow.is_empty() {
                line_number += 1;
                if line_number >= offset && lines_output < limit {
                    emit_line(
                        &overflow,
                        line_number,
                        line_number_width,
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
            // Find the next newline to delimit the current line.
            if let Some(newline_offset) = memchr(b'\n', &buf[pos..]) {
                let newline_pos = pos + newline_offset;
                line_number += 1;

                // Only format and append lines inside the requested window.
                if line_number >= offset && lines_output < limit {
                    if overflow.is_empty() {
                        // Fast path: entire line lives in this buffer.
                        emit_line(
                            &buf[pos..newline_pos],
                            line_number,
                            line_number_width,
                            &mut output,
                            &mut lines_output,
                            max_line_length,
                            line_numbers,
                        );
                    } else {
                        // Slow path: assemble the line from the overflow
                        // fragment buffered across a prior chunk boundary.
                        overflow.extend_from_slice(&buf[pos..newline_pos]);
                        emit_line(
                            &overflow,
                            line_number,
                            line_number_width,
                            &mut output,
                            &mut lines_output,
                            max_line_length,
                            line_numbers,
                        );
                        overflow.clear();
                    }
                } else if !overflow.is_empty() {
                    // Discard overflow for lines we're skipping past.
                    overflow.clear();
                }

                // Advance past the newline character.
                pos = newline_pos + 1;

                // Stop scanning this buffer once we've output enough lines.
                if lines_output >= limit {
                    break;
                }
            } else {
                // No newline in the remainder - buffer the partial line
                // and wait for the next chunk to complete it.
                overflow.extend_from_slice(&buf[pos..]);
                pos = buf.len();
            }
        }

        // Tell the buffered reader how much of this chunk we consumed.
        reader.consume(pos);

        if lines_output >= limit {
            break;
        }
    }

    // If the skip phase consumed the entire file, report the out-of-bounds
    // offset with the actual line count for a useful error message.
    if line_number < offset {
        return Err(ToolError::OutOfBounds(format!(
            "offset {} exceeds file length of {} lines",
            offset, line_number
        )));
    }

    Ok(ToolOutput::new(output))
}

/// Advances a buffered reader by counting `skip_target` newline boundaries
/// using SIMD-accelerated [`memchr_iter`] scanning, without processing line
/// content.
///
/// This avoids the per-line overhead of CR-stripping, UTF-8 validation, and
/// output formatting for skipped lines. The reader is left positioned at the
/// start of the next line after the skip target.
///
/// # Returns
/// The number of newline boundaries actually counted (may be less than
/// `skip_target` if EOF is reached first).
///
/// # Errors
/// Returns an I/O error if reading from the underlying reader fails.
#[maybe_async::maybe_async]
pub(crate) async fn skip_to_line(reader: &mut BufFile, skip_target: usize) -> ToolResult<usize> {
    // Import the sync or async `fill_buf`/`consume` trait depending on feature flag.
    #[cfg(feature = "blocking")]
    use std::io::BufRead as _;
    #[cfg(feature = "tokio")]
    use tokio::io::AsyncBufReadExt as _;

    let mut line_number = 0usize;
    // Track whether the buffer ended with non-newline bytes so we can count the
    // last unterminated line when EOF is reached.
    let mut trailing_content = false;
    while line_number < skip_target {
        // Determine how many buffered bytes to consume in this iteration.
        let consume = {
            let buf = reader.fill_buf().await?;
            if buf.is_empty() {
                // EOF reached - if the file ended without a trailing newline,
                // count the partial last line.
                if trailing_content {
                    line_number += 1;
                }
                break;
            }
            let remaining = skip_target - line_number;
            // Scan for newlines in the current buffer using SIMD-accelerated memchr.
            let mut count = 0usize;
            let mut last_pos = 0usize;
            for pos in memchr_iter(b'\n', buf) {
                count += 1;
                last_pos = pos;
                if count >= remaining {
                    break;
                }
            }
            line_number += count;
            if count >= remaining {
                // Found enough newlines - consume up to (and including) the one
                // that lands us on the target line.
                trailing_content = false;
                last_pos + 1
            } else {
                // Not enough newlines in this buffer — consume everything and
                // note whether the buffer ends without a newline.
                trailing_content = buf.last() != Some(&b'\n');
                buf.len()
            }
        };
        // Advance the reader past the consumed bytes.
        reader.consume(consume);
    }
    Ok(line_number)
}

/// Dispatches to the correct const-generic `process_line` monomorphization.
#[inline(always)]
fn emit_line(
    line_bytes: &[u8],
    line_number: usize,
    line_number_width: usize,
    output: &mut String,
    lines_output: &mut usize,
    max_line_length: usize,
    line_numbers: bool,
) {
    if line_numbers {
        process_line::<true>(
            line_bytes,
            line_number,
            line_number_width,
            output,
            lines_output,
            max_line_length,
        );
    } else {
        process_line::<false>(
            line_bytes,
            line_number,
            line_number_width,
            output,
            lines_output,
            max_line_length,
        );
    }
}

/// Processes a single line, appending it to output with optional line numbers.
///
/// Const-generic over `LINE_NUMBERS` so the compiler can eliminate the
/// branch at compile time and monomorphize two tight loops.
#[inline]
fn process_line<const LINE_NUMBERS: bool>(
    line_bytes: &[u8],
    line_number: usize,
    line_number_width: usize,
    output: &mut String,
    lines_output: &mut usize,
    max_line_length: usize,
) {
    let line_bytes = strip_cr(line_bytes);

    if *lines_output > 0 {
        output.push('\n');
    }

    if LINE_NUMBERS {
        push_padded_usize(output, line_number, line_number_width);
        output.push_str(": ");
    }

    // ASCII fast path: SIMD-accelerated check avoids full UTF-8 validation.
    // SAFETY: ASCII is always valid UTF-8.
    if line_bytes.is_ascii() {
        let content = unsafe { core::str::from_utf8_unchecked(line_bytes) };
        append_line_content(output, content, max_line_length);
    } else if let Ok(content) = core::str::from_utf8(line_bytes) {
        append_line_content(output, content, max_line_length);
    } else {
        let content = String::from_utf8_lossy(line_bytes);
        append_line_content(output, &content, max_line_length);
    }

    *lines_output += 1;
}

/// Strips trailing CR from a line (for CRLF handling).
#[inline]
fn strip_cr(line: &[u8]) -> &[u8] {
    if line.last() == Some(&b'\r') {
        &line[..line.len() - 1]
    } else {
        line
    }
}

#[inline]
fn append_line_content(output: &mut String, content: &str, max_line_length: usize) {
    let (display_content, was_truncated) = truncate_line_with_ellipsis(content, max_line_length);
    output.push_str(display_content);
    if was_truncated {
        output.push_str(TRUNCATION_ELLIPSIS);
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
            &settings,
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
        // With limit=2000, last_line=2000, width=4: "   1: hello\n   2: world"
        assert_eq!(result.content, "   1: hello\n   2: world");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn reads_basic_file_without_line_numbers() {
        let result = read_temp_file(b"hello\nworld\n", 1, 2000, false)
            .await
            .unwrap();
        assert_eq!(result.content, "hello\nworld");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn reads_file_with_multi_digit_line_numbers() {
        // 12-line file with limit=12: last_line=12, width=2, so line 1 is " 1:" (space-padded)
        let content = (1..=12).map(|i| format!("line{i}\n")).collect::<String>();
        let result = read_temp_file(content.as_bytes(), 1, 12, true)
            .await
            .unwrap();
        assert!(
            result.content.contains(" 1: line1"),
            "Expected padded ' 1: line1'"
        );
        assert!(
            result.content.contains("12: line12"),
            "Expected unpadded '12: line12'"
        );
        assert!(!result.content.contains("L"), "No 'L' prefix should appear");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn errors_on_offset_zero() {
        let err = read_temp_file(b"test\n", 0, 10, true).await.unwrap_err();
        assert!(matches!(err, ToolError::OutOfBounds(_)));
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn out_of_bounds_reports_correct_line_count_without_trailing_newline() {
        let err = read_temp_file(b"line1\nline2\nline3", 5, 10, true)
            .await
            .unwrap_err();
        let msg = match &err {
            ToolError::OutOfBounds(msg) => msg.clone(),
            other => panic!("expected OutOfBounds, got: {other:?}"),
        };
        assert!(
            msg.contains("3 lines"),
            "expected '3 lines' in error, got: {msg}"
        );
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
            &settings,
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
            &settings,
        )
        .await
        .unwrap();

        assert_eq!(result.content, "1: line1");
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
            &settings,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, ToolError::Validation { .. }));
    }
}
