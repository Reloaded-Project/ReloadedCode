//! YAML frontmatter preprocessor for unquoted colon-containing values.
//!
//! This module rewrites ambiguous inline YAML values such as
//! `model: provider/model:tag` into block scalars so they parse as literal
//! strings instead of nested mappings.
//!
//! # Problem
//!
//! YAML interprets `:` as a key-value separator. Values like
//! `provider/model:tag` or `http://localhost:8080` can be misparsed when they
//! are unquoted.
//!
//! # Transformations
//!
//! **Converted to block scalar** (value contains unquoted colon):
//!
//! ```text
//! Input:
//! model: provider/model:tag
//! api_url: http://localhost:8080
//!
//! Output:
//! model: |-
//!   provider/model:tag
//! api_url: |-
//!   http://localhost:8080
//! ```
//!
//! **Preserved unchanged** (already safe for YAML parsing):
//!
//! ```text
//! Input:
//! # comment: with:colon           # Comments are ignored
//! description: No colons here     # No colon in value
//! model: "provider/model:tag"     # Double-quoted
//! model: 'provider/model:tag'     # Single-quoted
//! content: |                      # Block scalar indicator
//!   line:with:colon
//! items: ["a:b", "c:d"]           # Flow array syntax
//! config: { "key": "a:b" }        # Flow mapping syntax
//!
//! Output: (identical to input)
//! ```
//!
//! # Notes
//!
//! - Uses `|-` (literal block, strip chomp) to avoid trailing newlines in values.
//! - Input is expected to be LF-normalized.
//! - Output uses LF line endings.
//! - This matches OpenCode's `preprocessFrontmatter` behavior.

use std::borrow::Cow;

/// Rewrites ambiguous frontmatter values so YAML parsing stays unambiguous.
pub(super) fn preprocess_frontmatter_yaml(input: &str) -> Cow<'_, str> {
    if input.is_empty() {
        return Cow::Borrowed(input);
    }

    match convert_block_scalars(input) {
        Some(output) => Cow::Owned(output),
        None => Cow::Borrowed(input),
    }
}

/// Rewrites matching lines and returns `None` when no rewrite is needed.
fn convert_block_scalars(input: &str) -> Option<String> {
    let first = match find_first_block_scalar(input) {
        Some(first) => first,
        _ => return None,
    };

    // Second pass: copy prefix once, then rewrite from the first changed line onward.
    // Typical rewrite is:
    //    `model: synthetic/hf:moonshotai/Kimi-K2.5`
    // -> `model: |-\n  synthetic/hf:moonshotai/Kimi-K2.5`
    // Another multi-colon example:
    //    `api_url: http://localhost:8080`
    // -> `api_url: |-\n  http://localhost:8080`
    // This also represents a change of +5 characters.
    let mut out = String::with_capacity(input.len() + 5);
    if first.line_start > 0 {
        out.push_str(&input[..first.line_start]);
    }
    out.push_str(first.key);
    out.push_str(": |-\n  ");
    out.push_str(first.value);

    // NOTE: `split_terminator('\n')` drops trailing empties, so rewrites may omit
    // a final `\n`; this is harmless because YAML deserialization is unaffected.
    for line in input[first.rest_start..].split_terminator('\n') {
        out.push('\n');
        if let Some((key, value)) = extract_if_needs_block_scalar(line) {
            out.push_str(key);
            out.push_str(": |-\n  ");
            out.push_str(value);
        } else {
            out.push_str(line);
        }
    }

    Some(out)
}

struct FirstBlockScalar<'a> {
    line_start: usize,
    rest_start: usize,
    key: &'a str,
    value: &'a str,
}

/// Finds the first line that must be rewritten and returns its offsets.
fn find_first_block_scalar(input: &str) -> Option<FirstBlockScalar<'_>> {
    let input_len = input.len();
    let mut line_start_offset = 0usize;

    for line in input.split_terminator('\n') {
        if let Some((key, value)) = extract_if_needs_block_scalar(line) {
            let mut rest_start = line_start_offset + line.len();
            if rest_start < input_len {
                rest_start += 1;
            }
            return Some(FirstBlockScalar {
                line_start: line_start_offset,
                rest_start,
                key,
                value,
            });
        }

        line_start_offset += line.len();
        if line_start_offset < input_len {
            line_start_offset += 1;
        }
    }

    None
}

/// Extracts key/value when a line needs transformation to block scalar format.
#[inline]
fn extract_if_needs_block_scalar(line: &str) -> Option<(&str, &str)> {
    // Ignore blank lines and YAML comments.
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    // Skip indented lines (usually continuation lines).
    let first = *line.as_bytes().first()?;
    if first == b' ' || first == b'\t' {
        return None;
    }

    // Split into key/value and validate the key shape.
    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim();
    if !is_valid_key(key) {
        return None;
    }

    // Leave already-safe value forms untouched.
    let value = line[colon_pos + 1..].trim();
    if value.is_empty() || value == ">" || value == "|" || value == "|-" || value == ">-" {
        return None;
    }

    // Quoted values are already safe, so we should not transform them.
    let first_value = value.as_bytes().first().copied();
    if matches!(first_value, Some(b'"') | Some(b'\'')) {
        return None;
    }

    if matches!(first_value, Some(b'{') | Some(b'[')) {
        return None;
    }

    // Skip YAML anchors, aliases, and tags - transforming these could change semantics.
    if matches!(first_value, Some(b'&') | Some(b'*') | Some(b'!')) {
        return None;
    }

    let bytes = value.as_bytes();
    let mut hash_idx = None;
    let mut has_colon = false;

    // Scan once up to the first inline comment marker.
    // We only care about ':' that appears before '#'.
    for (idx, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b':' => has_colon = true,
            b'#' => {
                hash_idx = Some(idx);
                break;
            }
            _ => {}
        }
    }

    let value = if let Some(hash_idx) = hash_idx {
        // If the comment suffix has ':', we treat the line as ambiguous
        // and leave it untouched to avoid false positives.
        if bytes[hash_idx + 1..].contains(&b':') {
            return None;
        }

        // Strip inline comment text from the transformed value.
        let val_part = value[..hash_idx].trim();
        if val_part.is_empty() || !has_colon {
            return None;
        }
        val_part
    } else {
        if !has_colon {
            return None;
        }
        value
    };

    // This line is ambiguous and should become a block scalar.
    Some((key, value))
}

/// Returns true when a key matches the simple identifier format we accept.
#[inline]
fn is_valid_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    rest.iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crlf_to_lf_inplace::crlf_to_lf_inplace;
    use indoc::indoc;
    use rstest::rstest;

    /// Verifies that ambiguous YAML values are rewritten as block scalars.
    #[rstest]
    // Unquoted colon inside a plain scalar must become a block scalar.
    #[case::colons_in_value(
        indoc! {"
            model: provider/model:tag
        "},
        false,
        indoc! {"
            model: |-
              provider/model:tag
        "},
        None
    )]
    // Key spacing still normalizes to `key: |-`.
    #[case::whitespace_around_key_separator(
        indoc! {"
            model : provider/model:tag
        "},
        false,
        indoc! {"
            model: |-
              provider/model:tag
        "},
        None
    )]
    // CRLF input is normalized before rewrite and both ambiguous values still transform.
    #[case::crlf_normalized_two_line_frontmatter(
        "model: provider/model:tag\r\napi_url: http://localhost:8080",
        true,
        "model: |-\n  provider/model:tag\napi_url: |-\n  http://localhost:8080",
        None
    )]
    // Transformed output drops the inline comment suffix from the scalar content.
    #[case::strips_inline_comment_when_rewriting(
        indoc! {"
            model: provider/model:tag # inline comment
        "},
        false,
        indoc! {"
            model: |-
              provider/model:tag
        "},
        Some("# inline comment")
    )]
    fn preprocess_transforms_to_block_scalar(
        #[case] raw_input: &str,
        #[case] normalize_crlf: bool,
        #[case] expected_fragment: &str,
        #[case] forbidden_fragment: Option<&str>,
    ) {
        let mut input = raw_input.to_string();
        if normalize_crlf {
            // Keep the explicit normalization path covered because the parser expects LF input.
            crlf_to_lf_inplace(&mut input);
        }

        let output = preprocess_frontmatter_yaml(&input);
        assert!(output.as_ref().contains(expected_fragment.trim_end()));

        if let Some(forbidden_fragment) = forbidden_fragment {
            assert!(!output.as_ref().contains(forbidden_fragment));
        }
    }

    /// Verifies that safe YAML constructs are preserved unchanged.
    #[rstest]
    // Quoted scalars are already YAML-safe.
    #[case::quoted_value("model: \"provider/model:tag\"")]
    // Block scalar indicators must not be rewritten again.
    #[case::existing_block_scalar(indoc! {"
        desc: |
          multiline
    "})]
    // Comment lines with colons are preserved verbatim.
    #[case::comment_line_is_ignored(indoc! {"
        # comment: with:colon
        mode: subagent
    "})]
    // Flow syntax is already explicit YAML and must stay untouched.
    #[case::flow_mapping("task: { \"*\": \"deny\" }")]
    #[case::flow_array("items: [\"a:b\", \"c:d\"]")]
    // Nested lines inside a block scalar are not treated as top-level keys.
    #[case::indented_continuation_line(indoc! {"
        desc: |
          line:with:colons
    "})]
    // A colon inside the inline comment keeps the original line unchanged to avoid false positives.
    #[case::ambiguous_inline_comment_with_colon("model: provider/model # note: keep")]
    fn preprocess_preserves_unchanged(#[case] input: &str) {
        let output = preprocess_frontmatter_yaml(input);
        assert_eq!(input, output.as_ref());
    }
}
