//! Shared utilities for tool implementations.

/// Generous estimate of average characters per line for buffer pre-allocation.
pub const ESTIMATED_CHARS_PER_LINE: usize = 64;

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

/// Truncates text to a maximum byte length, appending a truncation notice.
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

/// Truncates a single line to a maximum character count.
pub fn truncate_line(line: &str, max_chars: usize) -> (&str, bool) {
    // Fast path: UTF-8 guarantees byte_count >= char_count,
    // so if byte length fits, no truncation needed.
    if line.len() <= max_chars {
        return (line, false);
    }

    // Find byte position at max_chars character boundary
    let Some((byte_pos, _)) = line.char_indices().nth(max_chars) else {
        // Fewer than max_chars characters exist
        return (line, false);
    };

    (&line[..byte_pos], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_numbered_line_pads_correctly() {
        assert_eq!(format_numbered_line(1, "hello", 9), "1\thello");
        assert_eq!(format_numbered_line(1, "hello", 10), " 1\thello");
        assert_eq!(format_numbered_line(1, "hello", 100), "  1\thello");
    }

    #[test]
    fn truncate_text_preserves_short_text() {
        let (text, truncated) = truncate_text("hello", 10);
        assert_eq!(text, "hello");
        assert!(!truncated);
    }

    #[test]
    fn truncate_text_truncates_long_text() {
        let (text, truncated) = truncate_text("hello world", 5);
        assert_eq!(text, "hello");
        assert!(truncated);
    }

    #[test]
    fn truncate_text_respects_utf8_boundaries() {
        // "héllo" has é which is 2 bytes
        let (text, truncated) = truncate_text("héllo", 2);
        assert_eq!(text, "h");
        assert!(truncated);
    }

    #[test]
    fn truncate_line_preserves_short_line() {
        let (line, truncated) = truncate_line("hello", 10);
        assert_eq!(line, "hello");
        assert!(!truncated);
    }

    #[test]
    fn truncate_line_truncates_by_char_count() {
        let (line, truncated) = truncate_line("héllo", 3);
        assert_eq!(line, "hél");
        assert!(truncated);
    }
}
