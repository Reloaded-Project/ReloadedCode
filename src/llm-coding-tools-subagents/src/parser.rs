//! Agent markdown parser for files with YAML frontmatter headers.

use crlf_to_lf_inplace::crlf_to_lf_inplace;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Parser error variants independent of file paths.
#[derive(Debug, Error)]
pub enum AgentParseError {
    /// No frontmatter delimiters found in content.
    #[error("missing frontmatter")]
    MissingFrontmatter,

    /// YAML parsing failed.
    #[error("invalid YAML frontmatter: {message}")]
    InvalidYaml {
        /// YAML parser error message.
        message: String,
    },
}

/// Result of parsing a markdown file with frontmatter.
#[derive(Debug, Clone)]
pub(crate) struct AgentParseResult<T> {
    /// Parsed frontmatter data.
    pub(crate) data: T,
    /// Markdown content after frontmatter, trimmed of leading/trailing whitespace.
    /// Line endings are normalized to LF.
    pub(crate) content: String,
}

/// Path-free agent parsing function.
pub(crate) fn parse_agent<T: DeserializeOwned>(
    mut content: String,
) -> Result<AgentParseResult<T>, AgentParseError> {
    crlf_to_lf_inplace(&mut content);
    let Some(offsets) = find_frontmatter_offsets(&content) else {
        return Err(AgentParseError::MissingFrontmatter);
    };

    // Process YAML while we can still borrow content
    let yaml = &content[offsets.yaml_start..offsets.yaml_end];
    let yaml_preprocessed = preprocess_frontmatter_yaml(yaml);
    let data: T = serde_yaml::from_str(yaml_preprocessed.as_str()).map_err(|e| {
        AgentParseError::InvalidYaml {
            message: e.to_string(),
        }
    })?;

    // Extract body in-place (avoids reallocation)
    let body = extract_body_inplace(&mut content, offsets.body_start);

    Ok(AgentParseResult {
        data,
        content: body,
    })
}

#[derive(Clone, Copy)]
struct FrontmatterOffsets {
    yaml_start: usize,
    yaml_end: usize,
    body_start: usize,
}

#[inline]
fn find_frontmatter_offsets(content: &str) -> Option<FrontmatterOffsets> {
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
    let yaml_end = closing_newline;

    let yaml_start = tail
        .find('\n')
        .map(|n| after_opener + n + 1)
        .unwrap_or(after_opener);

    // Byte index at the start of the closing "---" delimiter
    let closing_start = closing_newline + 1;
    // Byte index after the closing "---" delimiter
    let after_closing = closing_start + 3;
    let mut body_start = after_closing;
    if after_closing < content.len() {
        let rest = &content.as_bytes()[after_closing..];
        if rest.starts_with(b"\n") {
            body_start += 1;
        }
    }

    Some(FrontmatterOffsets {
        yaml_start: yaml_start.min(yaml_end),
        yaml_end,
        body_start,
    })
}

/// Extracts the body from the content string in-place.
/// Splits off the body portion and trims whitespace without reallocation.
#[inline]
fn extract_body_inplace(content: &mut String, body_start: usize) -> String {
    if body_start >= content.len() {
        return String::new();
    }

    // Split off the body portion (from body_start to end)
    let mut body = content.split_off(body_start);

    // Trim leading whitespace in place
    let leading = body.bytes().take_while(|b| b.is_ascii_whitespace()).count();
    if leading > 0 {
        body.drain(..leading);
    }

    // Trim trailing whitespace in place
    let trailing = body
        .bytes()
        .rev()
        .take_while(|b| b.is_ascii_whitespace())
        .count();
    if trailing > 0 {
        body.truncate(body.len() - trailing);
    }

    body
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

/// Checks if a YAML line contains an unquoted colon in the value that needs
/// block scalar conversion.
///
/// Returns `Some((key, value))` if the line should be converted to block scalar
/// format, `None` if it's already safe for YAML parsing.
///
/// # Returns `None` (no conversion needed) when:
///
/// - Line is empty or a comment (`# ...`)
/// - Line is indented (continuation of a block scalar)
/// - No colon found (not a key-value pair)
/// - Key is not a valid YAML identifier
/// - Value is empty or already a block scalar indicator (`|`, `>`, `|-`, `>-`)
/// - Value is quoted (`"..."` or `'...'`)
/// - Value is a flow sequence (`[...]`) or mapping (`{...}`)
/// - Value doesn't contain a colon (no ambiguity to fix)
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
/// - Input is expected to be LF-normalized.
/// - Output uses LF line endings.
/// - This matches OpenCode's `preprocessFrontmatter` behavior.
fn preprocess_frontmatter_yaml(input: &str) -> YamlPreprocessed<'_> {
    if input.is_empty() {
        return YamlPreprocessed::Borrowed(input);
    }

    let converted = convert_block_scalars(input);
    match converted {
        Some(output) => YamlPreprocessed::Owned(output),
        None => YamlPreprocessed::Borrowed(input),
    }
}

enum YamlPreprocessed<'a> {
    Borrowed(&'a str),
    Owned(String),
}

impl YamlPreprocessed<'_> {
    #[inline]
    fn as_str(&self) -> &str {
        match self {
            YamlPreprocessed::Borrowed(value) => value,
            YamlPreprocessed::Owned(value) => value.as_str(),
        }
    }
}

/// Converts lines with unquoted colons in values to block scalar format.
/// Returns `None` when no conversion is needed.
fn convert_block_scalars(input: &str) -> Option<String> {
    let input_len = input.len();
    let mut output: Option<String> = None;
    let mut need_newline = false;
    let mut offset = 0usize;

    for line in input.split_terminator('\n') {
        if let Some(out) = output.as_mut() {
            if need_newline {
                out.push('\n');
            }
            if let Some((key, value)) = block_scalar_parts(line) {
                out.push_str(key);
                out.push_str(": |-\n  ");
                out.push_str(value);
            } else {
                out.push_str(line);
            }
            need_newline = true;
        } else if let Some((key, value)) = block_scalar_parts(line) {
            let mut out = String::with_capacity(input_len + 3);
            if offset > 0 {
                out.push_str(&input[..offset]);
            }
            out.push_str(key);
            out.push_str(": |-\n  ");
            out.push_str(value);
            output = Some(out);
            need_newline = true;
        }

        offset += line.len();
        if offset < input_len {
            offset += 1;
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RawFrontmatter;

    #[test]
    fn preprocess_handles_colons_in_value() {
        let input = "model: provider/model:tag";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.as_str().contains("model: |-"));
        assert!(output.as_str().contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_preserves_quoted_values() {
        let input = "model: \"provider/model:tag\"";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.as_str().contains("model: \"provider/model:tag\""));
    }

    #[test]
    fn preprocess_preserves_block_scalars() {
        let input = "desc: |\n  multiline";
        let output = preprocess_frontmatter_yaml(input);
        assert_eq!(input, output.as_str());
    }

    #[test]
    fn preprocess_skips_comments() {
        let input = "# comment: with:colon\nmode: subagent";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.as_str().contains("# comment: with:colon"));
    }

    #[test]
    fn preprocess_skips_flow_mappings() {
        let input = "task: { \"*\": \"deny\" }";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.as_str().contains("task: { \"*\": \"deny\" }"));
    }

    #[test]
    fn preprocess_skips_flow_arrays() {
        let input = "items: [\"a:b\", \"c:d\"]";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.as_str().contains("items: [\"a:b\", \"c:d\"]"));
    }

    #[test]
    fn preprocess_handles_key_with_whitespace_around_colon() {
        let input = "model : provider/model:tag";
        let output = preprocess_frontmatter_yaml(input);
        assert!(output.as_str().contains("model: |-"));
        assert!(output.as_str().contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_handles_crlf_line_endings() {
        let mut input = "model: provider/model:tag\r\napi_url: http://localhost:8080".to_string();
        crlf_to_lf_inplace(&mut input);
        let output = preprocess_frontmatter_yaml(&input);
        assert!(output.as_str().contains("model: |-"));
        assert!(output.as_str().contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_skips_indented_lines() {
        // FIX #1: Indented lines should be skipped (continuation of previous value)
        let input = "desc: |\n  line:with:colons";
        let output = preprocess_frontmatter_yaml(input);
        // Should NOT convert the indented line
        assert!(output.as_str().contains("  line:with:colons"));
        assert!(!output.as_str().contains("  line: |-")); // Should not have nested block scalar
    }

    #[test]
    fn parse_extracts_frontmatter_and_content() {
        let input = "---\nmode: subagent\ndescription: Test agent\n---\n\nPrompt body here.";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.data.description, Some("Test agent".to_string()));
        assert_eq!(result.content, "Prompt body here.");
    }

    #[test]
    fn parse_trims_body_whitespace() {
        let input = "---\nmode: primary\n---\n\n  indented\n\ntrailing\n";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "indented\n\ntrailing");
    }

    #[test]
    fn parse_handles_empty_body() {
        let input = "---\nmode: primary\n---";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert!(result.content.is_empty());
    }

    #[test]
    fn parse_handles_empty_frontmatter() {
        // FIX #2: Handle ---\n--- case (empty YAML)
        let input = "---\n---\nbody";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_handles_whitespace_only_frontmatter() {
        // FIX #2: Handle frontmatter with only whitespace
        let input = "---\n  \n---\nbody";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_trims_crlf_in_body() {
        // FIX #3: Body should normalize CRLF to LF
        let input = "---\nmode: subagent\n---\nline1\r\nline2\r\n";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "line1\nline2");
    }

    #[test]
    fn parse_trims_crlf_body_with_crlf_frontmatter() {
        // FIX #3: CRLF in frontmatter should normalize body
        let input = "---\r\nmode: subagent\r\n---\r\nbody\r\nline2\r\n";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body\nline2");
    }

    #[test]
    fn parse_rejects_frontmatter_not_at_start() {
        let input = "some text\n---\nmode: subagent\n---\nbody";
        let result: Result<AgentParseResult<RawFrontmatter>, AgentParseError> =
            parse_agent(input.to_string());

        assert!(matches!(result, Err(AgentParseError::MissingFrontmatter)));
    }

    #[test]
    fn parse_handles_bom() {
        let input = "\u{FEFF}---\nmode: subagent\n---\nbody";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_returns_error_for_missing_frontmatter() {
        let input = "No frontmatter here";
        let result: Result<AgentParseResult<RawFrontmatter>, AgentParseError> =
            parse_agent(input.to_string());

        assert!(matches!(result, Err(AgentParseError::MissingFrontmatter)));
    }

    #[test]
    fn parse_returns_error_for_invalid_yaml() {
        let input = "---\n[invalid yaml\n---\nbody";
        let result: Result<AgentParseResult<RawFrontmatter>, AgentParseError> =
            parse_agent(input.to_string());

        assert!(matches!(result, Err(AgentParseError::InvalidYaml { .. })));
    }

    #[test]
    fn block_scalar_no_trailing_newline() {
        let input = "---\nmodel: provider/model:tag\n---\nbody";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        // Model should NOT have trailing newline
        assert_eq!(result.data.model, Some("provider/model:tag".to_string()));
    }

    #[test]
    fn parse_error_display_messages() {
        let cases = [
            (AgentParseError::MissingFrontmatter, "missing frontmatter"),
            (
                AgentParseError::InvalidYaml {
                    message: "bad".to_string(),
                },
                "invalid YAML frontmatter: bad",
            ),
        ];

        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }
}
