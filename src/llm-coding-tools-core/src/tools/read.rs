//! File reading operation.

use crate::error::{ToolError, ToolResult};
use crate::fs;
use crate::output::ToolOutput;
use crate::path::PathResolver;
use crate::util::{
    truncate_line_with_ellipsis, ESTIMATED_CHARS_PER_LINE, MIN_LINE_LENGTH, TRUNCATION_ELLIPSIS,
};
use memchr::memchr;
use std::borrow::Cow;
use std::fmt::Write;

/// Strips trailing CR from a line (for CRLF handling).
#[inline]
fn strip_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Processes a single line, appending it to output with optional line numbers.
#[inline]
fn process_line<const LINE_NUMBERS: bool>(
    line_bytes: &[u8],
    line_number: usize,
    output: &mut String,
    lines_output: &mut usize,
    max_line_length: usize,
) {
    let line_bytes = strip_cr(line_bytes);
    let content: Cow<'_, str> = String::from_utf8_lossy(line_bytes);
    let (display_content, was_truncated) = truncate_line_with_ellipsis(&content, max_line_length);

    if *lines_output > 0 {
        output.push('\n');
    }

    if LINE_NUMBERS {
        let _ = write!(output, "L{}: {}", line_number, display_content);
    } else {
        output.push_str(display_content);
    }

    if was_truncated {
        output.push_str(TRUNCATION_ELLIPSIS);
    }

    *lines_output += 1;
}

/// Reads a file and returns formatted content, optionally with line numbers.
///
/// When `LINE_NUMBERS` is `true`, each line is prefixed with `L{number}: `.
/// When `false`, raw content is returned without prefixes.
#[maybe_async::maybe_async]
pub async fn read_file<R: PathResolver, const LINE_NUMBERS: bool>(
    resolver: &R,
    file_path: &str,
    offset: usize,
    limit: usize,
    max_line_length: usize,
) -> ToolResult<ToolOutput> {
    // Conditional trait import for consume() method
    #[cfg(feature = "blocking")]
    use std::io::BufRead as _;
    #[cfg(feature = "tokio")]
    use tokio::io::AsyncBufReadExt as _;

    if offset == 0 {
        return Err(ToolError::OutOfBounds(
            "offset must be >= 1 (1-indexed)".into(),
        ));
    }
    if limit == 0 {
        return Err(ToolError::OutOfBounds("limit must be >= 1".into()));
    }
    if max_line_length < MIN_LINE_LENGTH {
        return Err(ToolError::Validation(format!(
            "max_line_length must be >= {}",
            MIN_LINE_LENGTH
        )));
    }

    let path = resolver.resolve(file_path)?;
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
                    process_line::<LINE_NUMBERS>(
                        &overflow,
                        line_number,
                        &mut output,
                        &mut lines_output,
                        max_line_length,
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
                        process_line::<LINE_NUMBERS>(
                            &buf[pos..newline_pos],
                            line_number,
                            &mut output,
                            &mut lines_output,
                            max_line_length,
                        );
                    } else {
                        // Slow path: prepend buffered fragment.
                        overflow.extend_from_slice(&buf[pos..newline_pos]);
                        process_line::<LINE_NUMBERS>(
                            &overflow,
                            line_number,
                            &mut output,
                            &mut lines_output,
                            max_line_length,
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
    use std::io::Write as _;
    use tempfile::NamedTempFile;

    #[maybe_async::maybe_async]
    async fn read_temp_file<const LINE_NUMBERS: bool>(
        content: &[u8],
        offset: usize,
        limit: usize,
    ) -> ToolResult<ToolOutput> {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(content).unwrap();
        let resolver = AbsolutePathResolver;
        read_file::<_, LINE_NUMBERS>(
            &resolver,
            temp.path().to_str().unwrap(),
            offset,
            limit,
            2000, // max_line_length
        )
        .await
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn reads_basic_file_with_line_numbers() {
        let result = read_temp_file::<true>(b"hello\nworld\n", 1, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "L1: hello\nL2: world");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn reads_basic_file_without_line_numbers() {
        let result = read_temp_file::<false>(b"hello\nworld\n", 1, 2000)
            .await
            .unwrap();
        assert_eq!(result.content, "hello\nworld");
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn errors_on_offset_zero() {
        let err = read_temp_file::<true>(b"test\n", 0, 10).await.unwrap_err();
        assert!(matches!(err, ToolError::OutOfBounds(_)));
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn errors_on_invalid_max_line_length() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"test\n").unwrap();
        let resolver = AbsolutePathResolver;

        let err = read_file::<_, true>(
            &resolver,
            temp.path().to_str().unwrap(),
            1,
            10,
            3, // below MIN_LINE_LENGTH of 4
        )
        .await
        .unwrap_err();

        assert!(matches!(err, ToolError::Validation(_)));
        assert!(err.to_string().contains("max_line_length must be >= 4"));
    }

    #[maybe_async::test(feature = "blocking", async(feature = "tokio", tokio::test))]
    async fn truncates_long_line_with_ellipsis() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"abcdefghij\n").unwrap();
        let resolver = AbsolutePathResolver;

        let result = read_file::<_, false>(
            &resolver,
            temp.path().to_str().unwrap(),
            1,
            10,
            6, // keep 3 chars + "..."
        )
        .await
        .unwrap();

        assert_eq!(result.content, "abc...");
    }
}
