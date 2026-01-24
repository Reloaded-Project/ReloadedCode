//! Frontmatter parsing for markdown files with YAML headers.

use crate::error::{AgentConfigError, AgentConfigResult};
use crlf_to_lf_inplace::crlf_to_lf_inplace;
use serde::de::DeserializeOwned;
use std::borrow::Cow;
use std::path::Path;

/// Result of parsing a markdown file with frontmatter.
#[derive(Debug, Clone)]
pub struct FrontmatterParseResult<T> {
    /// Parsed frontmatter data.
    pub data: T,
    /// Markdown content after frontmatter, trimmed of leading/trailing whitespace.
    pub content: String,
}

/// Parses a markdown file with YAML frontmatter.
///
/// The file must start with `---` (at position 0, optionally after BOM),
/// followed by YAML, followed by `---` on its own line.
/// Content after the closing `---` is the markdown body (trimmed at the edges).
///
/// # Errors
///
/// Returns [`AgentConfigError::MissingFrontmatter`] if no valid frontmatter found.
/// Returns [`AgentConfigError::InvalidYaml`] if YAML parsing fails.
pub fn parse_frontmatter<T: DeserializeOwned>(
    content: &str,
    path: &Path,
) -> AgentConfigResult<FrontmatterParseResult<T>> {
    let Some(parts) = split_frontmatter(content) else {
        return Err(AgentConfigError::MissingFrontmatter {
            path: path.to_path_buf(),
        });
    };

    let yaml_preprocessed = preprocess_frontmatter_yaml(parts.yaml);
    let data: T = serde_yaml::from_str(yaml_preprocessed.as_ref()).map_err(|e| {
        AgentConfigError::InvalidYaml {
            path: path.to_path_buf(),
            message: e.to_string(),
        }
    })?;

    let body = if parts.body.is_empty() {
        String::new()
    } else {
        parts.body.to_string()
    };

    Ok(FrontmatterParseResult {
        data,
        content: body,
    })
}

#[derive(Clone, Copy)]
struct FrontmatterSlices<'a> {
    yaml: &'a str,
    body: &'a str,
}

#[inline]
fn trim_ascii_whitespace(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut start = 0usize;
    let mut end = bytes.len();

    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    &input[start..end]
}

#[inline]
fn split_frontmatter(content: &str) -> Option<FrontmatterSlices<'_>> {
    let bytes = content.as_bytes();
    let bom_len = if content.starts_with('\u{FEFF}') {
        '\u{FEFF}'.len_utf8()
    } else {
        0
    };
    let start = &content[bom_len..];
    if !start.starts_with("---") {
        return None;
    }

    // Byte index after the opening "---" delimiter
    let after_opener = bom_len + 3;
    let tail = &content[after_opener..];
    let end_offset = tail.find("\n---")?;
    // Byte index of the newline before the closing "---"
    let closing_newline = after_opener + end_offset;
    let has_cr = closing_newline > 0 && bytes[closing_newline - 1] == b'\r';
    let yaml_end = if has_cr {
        closing_newline - 1
    } else {
        closing_newline
    };

    let yaml_start = tail
        .find('\n')
        .map(|n| after_opener + n + 1)
        .unwrap_or(after_opener);

    let yaml = if yaml_start <= yaml_end {
        &content[yaml_start..yaml_end]
    } else {
        ""
    };

    // Byte index at the start of the closing "---" delimiter
    let closing_start = closing_newline + 1;
    // Byte index after the closing "---" delimiter
    let after_closing = closing_start + 3;
    let mut body_start = after_closing;
    if after_closing < content.len() {
        let rest = &bytes[after_closing..];
        if has_cr {
            if rest.starts_with(b"\r\n") {
                body_start += 2;
            } else if rest.starts_with(b"\n") {
                body_start += 1;
            }
        } else if rest.starts_with(b"\n") {
            body_start += 1;
        } else if rest.starts_with(b"\r\n") {
            body_start += 2;
        }
    }
    let body = if body_start < content.len() {
        trim_ascii_whitespace(&content[body_start..])
    } else {
        ""
    };

    Some(FrontmatterSlices { yaml, body })
}

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

#[inline]
fn block_scalar_parts(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let first = *line.as_bytes().first()?;
    if first == b' ' || first == b'\t' {
        return None;
    }

    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim();
    if !is_valid_key(key) {
        return None;
    }

    let value = line[colon_pos + 1..].trim();
    if value.is_empty() || value == ">" || value == "|" || value == "|-" || value == ">-" {
        return None;
    }

    let first_value = value.as_bytes().first().copied();
    if matches!(first_value, Some(b'"') | Some(b'\'')) {
        return None;
    }

    if matches!(first_value, Some(b'{') | Some(b'[')) {
        return None;
    }

    if !value.contains(':') {
        return None;
    }

    Some((key, value))
}

/// Preprocesses YAML frontmatter to handle inline `key: value:with:colons`.
/// The input is the YAML slice only (no `---` delimiters).
///
/// # Problem
///
/// YAML interprets colons as key-value separators. A value like `provider/model:tag`
/// would be misparsed as a nested mapping. This function converts such lines to
/// block scalar format, which treats the entire value as a literal string.
///
/// # Transformations
///
/// **Converted to block scalar** (value contains unquoted colon):
///
/// ```text
/// Input:
/// model: provider/model:tag
/// api_url: http://localhost:8080
///
/// Output:
/// model: |-
///   provider/model:tag
/// api_url: |-
///   http://localhost:8080
/// ```
///
/// **Preserved unchanged** (already safe for YAML parsing):
///
/// ```text
/// Input:
/// # comment: with:colon           # Comments are ignored
/// description: No colons here     # No colon in value
/// model: "provider/model:tag"     # Double-quoted
/// model: 'provider/model:tag'     # Single-quoted
/// content: |                      # Block scalar indicator
///   line:with:colon
/// items: ["a:b", "c:d"]           # Flow array syntax
/// config: { "key": "a:b" }        # Flow mapping syntax
///
/// Output: (identical to input)
/// ```
///
/// # Notes
///
/// - Uses `|-` (literal block, strip chomp) to avoid trailing newlines in values.
/// - Normalizes CRLF to LF in output. Use only for YAML parsing; preserve
///   original content for the body.
/// - This matches OpenCode's `preprocessFrontmatter` behavior.
fn preprocess_frontmatter_yaml(input: &str) -> Cow<'_, str> {
    if input.is_empty() {
        return Cow::Borrowed(input);
    }

    // Phase 1: CRLF normalization using fast memchr detection
    let normalized: Cow<'_, str> = if memchr::memchr(b'\r', input.as_bytes()).is_some() {
        let mut s = input.to_string();
        crlf_to_lf_inplace(&mut s);
        Cow::Owned(s)
    } else {
        Cow::Borrowed(input)
    };

    // Phase 2: Block scalar conversion (input is now LF-only)
    convert_block_scalars(normalized)
}

/// Converts lines with unquoted colons in values to block scalar format.
/// Assumes input uses LF line endings only.
#[inline]
fn convert_block_scalars(input: Cow<'_, str>) -> Cow<'_, str> {
    // Fast path: check if any conversion is needed
    let needs_conversion = input.lines().any(|line| block_scalar_parts(line).is_some());

    if !needs_conversion {
        return input;
    }

    // Calculate output size: for each converted line, we add "|-\n  " (5 chars)
    // minus ": " (2 chars) = net +3 chars per conversion
    let conversion_count = input
        .lines()
        .filter(|l| block_scalar_parts(l).is_some())
        .count();
    let mut output = String::with_capacity(input.len() + conversion_count * 3);
    let mut first = true;

    for line in input.lines() {
        if !first {
            output.push('\n');
        } else {
            first = false;
        }

        if let Some((key, value)) = block_scalar_parts(line) {
            output.push_str(key);
            output.push_str(": |-\n  ");
            output.push_str(value);
        } else {
            output.push_str(line);
        }
    }

    Cow::Owned(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RawFrontmatter;

    #[test]
    fn preprocess_handles_colons_in_value() {
        let input = "model: provider/model:tag";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("model: |-"));
        assert!(output.contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_preserves_quoted_values() {
        let input = "model: \"provider/model:tag\"";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("model: \"provider/model:tag\""));
    }

    #[test]
    fn preprocess_preserves_block_scalars() {
        let input = "desc: |\n  multiline";
        let output = preprocess_frontmatter_yaml(input);
        assert_eq!(input, output.as_ref());
    }

    #[test]
    fn preprocess_skips_comments() {
        let input = "# comment: with:colon\nmode: subagent";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("# comment: with:colon"));
    }

    #[test]
    fn preprocess_skips_flow_mappings() {
        let input = "task: { \"*\": \"deny\" }";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("task: { \"*\": \"deny\" }"));
    }

    #[test]
    fn preprocess_skips_flow_arrays() {
        let input = "items: [\"a:b\", \"c:d\"]";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("items: [\"a:b\", \"c:d\"]"));
    }

    #[test]
    fn preprocess_handles_key_with_whitespace_around_colon() {
        let input = "model : provider/model:tag";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("model: |-"));
        assert!(output.contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_handles_crlf_line_endings() {
        let input = "model: provider/model:tag\r\napi_url: http://localhost:8080";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.contains("model: |-"));
        assert!(output.contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_skips_indented_lines() {
        // FIX #1: Indented lines should be skipped (continuation of previous value)
        let input = "desc: |\n  line:with:colons";
        let output = preprocess_frontmatter_yaml(input);
        // Should NOT convert the indented line
        assert!(output.contains("  line:with:colons"));
        assert!(!output.contains("  line: |-")); // Should not have nested block scalar
    }

    #[test]
    fn parse_extracts_frontmatter_and_content() {
        let input = "---\nmode: subagent\ndescription: Test agent\n---\n\nPrompt body here.";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.data.description, Some("Test agent".to_string()));
        assert_eq!(result.content, "Prompt body here.");
    }

    #[test]
    fn parse_trims_body_whitespace() {
        let input = "---\nmode: primary\n---\n\n  indented\n\ntrailing\n";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "indented\n\ntrailing");
    }

    #[test]
    fn parse_handles_empty_body() {
        let input = "---\nmode: primary\n---";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert!(result.content.is_empty());
    }

    #[test]
    fn parse_handles_empty_frontmatter() {
        // FIX #2: Handle ---\n--- case (empty YAML)
        let input = "---\n---\nbody";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_handles_whitespace_only_frontmatter() {
        // FIX #2: Handle frontmatter with only whitespace
        let input = "---\n  \n---\nbody";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_trims_crlf_in_body() {
        // FIX #3: Body should preserve CRLF line endings in content
        let input = "---\nmode: subagent\n---\nline1\r\nline2\r\n";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "line1\r\nline2");
    }

    #[test]
    fn parse_trims_crlf_body_with_crlf_frontmatter() {
        // FIX #3: CRLF in frontmatter should not affect body preservation
        let input = "---\r\nmode: subagent\r\n---\r\nbody\r\nline2\r\n";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "body\r\nline2");
    }

    #[test]
    fn parse_rejects_frontmatter_not_at_start() {
        let input = "some text\n---\nmode: subagent\n---\nbody";
        let result: AgentConfigResult<FrontmatterParseResult<RawFrontmatter>> =
            parse_frontmatter(input, Path::new("test.md"));

        assert!(matches!(
            result,
            Err(AgentConfigError::MissingFrontmatter { .. })
        ));
    }

    #[test]
    fn parse_handles_bom() {
        let input = "\u{FEFF}---\nmode: subagent\n---\nbody";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_returns_error_for_missing_frontmatter() {
        let input = "No frontmatter here";
        let result: AgentConfigResult<FrontmatterParseResult<RawFrontmatter>> =
            parse_frontmatter(input, Path::new("test.md"));

        assert!(matches!(
            result,
            Err(AgentConfigError::MissingFrontmatter { .. })
        ));
    }

    #[test]
    fn parse_returns_error_for_invalid_yaml() {
        let input = "---\n[invalid yaml\n---\nbody";
        let result: AgentConfigResult<FrontmatterParseResult<RawFrontmatter>> =
            parse_frontmatter(input, Path::new("test.md"));

        assert!(matches!(result, Err(AgentConfigError::InvalidYaml { .. })));
    }

    #[test]
    fn block_scalar_no_trailing_newline() {
        let input = "---\nmodel: provider/model:tag\n---\nbody";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        // Model should NOT have trailing newline
        assert_eq!(result.data.model, Some("provider/model:tag".to_string()));
    }
}
