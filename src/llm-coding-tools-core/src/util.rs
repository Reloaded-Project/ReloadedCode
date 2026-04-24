//! Shared utilities for tool implementations.

/// Generous estimate of average characters per line for buffer pre-allocation.
pub const ESTIMATED_CHARS_PER_LINE: usize = 64;

/// Suffix added to truncated lines.
pub(crate) const TRUNCATION_ELLIPSIS: &str = "...";

/// Minimum characters per output line when using `...` truncation.
pub const MIN_LINE_LENGTH: usize = TRUNCATION_ELLIPSIS.len() + 1;

/// Minimum value for limit/count fields (e.g., read.limit, grep.limit, glob.limit).
pub const MIN_LIMIT: usize = 1;

/// Minimum value for timeout fields in milliseconds (e.g., bash.timeout_ms, webfetch.timeout_ms).
pub const MIN_TIMEOUT_MS: u32 = 1000;

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

/// Appends `n` right-aligned with leading spaces to fill `width` characters.
/// E.g. `push_padded_usize(buf, 5, 4)` appends `"   5"`.
///
/// When `width` equals the digit count of `n`, this appends just the digits
/// (no padding), equivalent to a plain integer-to-string conversion.
///
/// # Safety (caller contract)
///
/// `width` must be >= the number of digits in `n`. This is guaranteed by
/// construction: callers compute `width` from the maximum line number or
/// from the number's own digit count.
#[inline]
pub(crate) fn push_padded_usize(output: &mut String, n: usize, width: usize) {
    debug_assert!(width <= 20, "width exceeds stack buffer");
    let mut buf = [b' '; 20];
    let mut pos = 20usize;
    let mut m = n;
    if m == 0 {
        pos -= 1;
        buf[pos] = b'0';
    } else {
        while m > 0 {
            pos -= 1;
            buf[pos] = b'0' + (m % 10) as u8;
            m /= 10;
        }
    }
    // `width >= digit_count(n)` by contract, so `20 - width <= pos`.
    // buf[20-width..pos] is already spaces; buf[pos..20] has digits.
    let start = 20 - width;
    debug_assert!(start <= pos, "width ({width}) < digit count of {n}");
    unsafe {
        output.push_str(core::str::from_utf8_unchecked(&buf[start..]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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

    #[rstest]
    #[case::single_digit_no_padding(5, 1, "5")]
    #[case::width_greater_than_digits(5, 4, "   5")]
    #[case::width_equals_digits(42, 2, "42")]
    #[case::zero(0, 1, "0")]
    #[case::zero_with_padding(0, 3, "  0")]
    #[case::large_number(999, 3, "999")]
    #[case::large_number_with_padding(123, 5, "  123")]
    fn push_padded_usize_should_right_align_with_spaces(
        #[case] n: usize,
        #[case] width: usize,
        #[case] expected: &str,
    ) {
        let mut output = String::new();
        push_padded_usize(&mut output, n, width);
        assert_eq!(output, expected);
    }
}
