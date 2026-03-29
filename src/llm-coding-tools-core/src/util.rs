//! Shared utilities for tool implementations.

/// Generous estimate of average characters per line for buffer pre-allocation.
pub const ESTIMATED_CHARS_PER_LINE: usize = 64;

/// Suffix added to truncated lines.
pub(crate) const TRUNCATION_ELLIPSIS: &str = "...";

/// Minimum characters per output line when using `...` truncation.
pub const MIN_LINE_LENGTH: usize = TRUNCATION_ELLIPSIS.len() + 1;

/// A number of characters per line that's likely to not be exceeded in most files.
pub const LIKELY_CHARS_PER_LINE_MAX: usize = ESTIMATED_CHARS_PER_LINE * 4;

/// Formats a line with its line number for output.
///
/// Uses the format: `{spaces}{line_number}\t{content}` where spaces
/// pad the line number to align with the widest number in the range.
#[inline]
pub fn format_numbered_line(line_number: usize, content: &str, max_line_number: usize) -> String {
    let width = max_line_number.checked_ilog10().unwrap_or(0) as usize + 1;
    format!("{:>width$}\t{}", line_number, content)
}

/// Truncates text to a maximum byte length at a UTF-8 boundary.
///
/// Returns `(truncated_text, was_truncated)`.
pub fn truncate_text(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }

    // Find a valid UTF-8 boundary before max_bytes
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    (&text[..end], true)
}

/// Truncates a line for display with a trailing [`TRUNCATION_ELLIPSIS`].
///
/// Returns `(prefix, was_truncated)` where callers should append
/// [`TRUNCATION_ELLIPSIS`] when `was_truncated` is `true`.
///
/// The returned `prefix` is sized so that `prefix + "..."` fits within
/// `max_chars`. Callers must pass `max_chars >= 4`.
///
/// Edge case: for invalid `max_chars < 4`, this function falls back to plain
/// character-count truncation (`prefix.len() <= max_chars` in chars).
#[inline]
pub(crate) fn truncate_line_with_ellipsis(line: &str, max_chars: usize) -> (&str, bool) {
    const ELLIPSIS_LEN: usize = TRUNCATION_ELLIPSIS.len();

    // Fast path: if byte length fits, char length must also fit.
    if line.len() <= max_chars {
        return (line, false);
    }

    // Defensive fallback for invalid settings where `max_chars < 4`.
    // If content exceeds the char limit, do plain char-count truncation.
    if max_chars <= ELLIPSIS_LEN {
        let Some((keep_byte, _)) = line.char_indices().nth(max_chars) else {
            return (line, false);
        };
        return (&line[..keep_byte], true);
    }

    let keep_chars = max_chars - ELLIPSIS_LEN;

    // Hot path for source-like text: if first max_chars+1 bytes are ASCII,
    // byte and character boundaries are identical.

    // ASCII check is fast (it's SIMD), and text being ASCII is the hot path
    // so almost always, this check is true.
    if line.as_bytes()[..max_chars + 1].is_ascii() {
        return (&line[..keep_chars], true);
    }

    let mut iter = line.char_indices();

    let Some((keep_byte, _)) = iter.nth(keep_chars) else {
        // More bytes than max_chars but no more than max_chars in UTF-8.
        return (line, false);
    };

    // Okay. truncate at keep_byte; if possible, that is.
    if iter.nth(ELLIPSIS_LEN - 1).is_some() {
        (&line[..keep_byte], true)
    } else {
        (line, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::single_digit_max(1, 9, "1\thello")] // 1-9, no padding needed
    #[case::double_digit_max(1, 10, " 1\thello")] // 10, 1 space padding
    #[case::triple_digit_max(1, 100, "  1\thello")] // 100, 2 space padding
    fn format_numbered_line_pads_correctly(
        #[case] line: usize,
        #[case] max: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(format_numbered_line(line, "hello", max), expected);
    }

    #[rstest]
    #[case::shorter_than_max("hello", 10, "hello", false)] // Text shorter than max, no truncation
    #[case::longer_than_max("hello world", 5, "hello", true)] // Text longer than max, truncates
    fn truncate_text_cases(
        #[case] input: &str,
        #[case] max: usize,
        #[case] expected: &str,
        #[case] was_truncated: bool,
    ) {
        let (text, truncated) = truncate_text(input, max);
        assert_eq!(text, expected);
        assert_eq!(truncated, was_truncated);
    }

    #[test]
    fn truncate_text_respects_utf8_boundaries() {
        // "héllo" has é which is 2 bytes - truncation must happen at char boundary, not byte boundary
        let (text, truncated) = truncate_text("héllo", 2);
        assert_eq!(text, "h");
        assert!(truncated);
    }

    #[rstest]
    #[case::short_line_preserved("hello", 10, "hello", false)] // Line shorter than max, preserved unchanged
    #[case::ascii_truncation("abcdefgh", 6, "abc", true)] // ASCII: keeps max-3 chars, adds "..."
    #[case::utf8_multi_byte("héllö", 4, "h", true)] // UTF-8: respects char boundaries (é=2 bytes)
    #[case::minimum_limit("abcdefgh", 4, "a", true)] // Min limit 4: keeps only 1 char + "..."
    #[case::short_utf8_preserved("ééé", 4, "ééé", false)] // 3 UTF-8 chars fit in limit, not truncated
    #[case::tiny_limit_fallback("éééé", 3, "ééé", true)] // Limit too small: fallback to char count
    fn truncate_line_with_ellipsis_cases(
        #[case] input: &str,
        #[case] max: usize,
        #[case] expected: &str,
        #[case] was_truncated: bool,
    ) {
        let (line, truncated) = truncate_line_with_ellipsis(input, max);
        assert_eq!(line, expected);
        assert_eq!(truncated, was_truncated);
    }
}
