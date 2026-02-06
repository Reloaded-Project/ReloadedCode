//! Agent markdown parser for files with YAML frontmatter.
//!
//! Parses markdown that starts with `---` frontmatter and returns deserialized
//! frontmatter data plus normalized body content (LF line endings, trimmed).
//! YAML frontmatter is preprocessed by the `preprocessor` module before
//! deserialization to handle unquoted colon-containing values safely.

mod preprocessor;

use crlf_to_lf_inplace::crlf_to_lf_inplace;
use preprocessor::preprocess_frontmatter_yaml;
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
    let data: T = serde_yaml::from_str(yaml_preprocessed.as_ref()).map_err(|e| {
        AgentParseError::InvalidYaml {
            message: e.to_string(),
        }
    })?;

    // Extract body by mutating and reusing the existing allocation.
    let body = extract_body_inplace(content, offsets.body_start);

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

/// Extracts the body by mutating the original string in-place.
/// Reuses the existing allocation and leaves only the trimmed body.
#[inline]
fn extract_body_inplace(mut content: String, body_start: usize) -> String {
    if body_start >= content.len() {
        content.clear();
        return content;
    }

    let len = content.len();
    let bytes = content.as_bytes();
    let mut start_offset = body_start;
    let mut end_offset = len;

    // UTF-8 byte classes:
    // | Range       | Meaning                  | `is_ascii_whitespace()`  |
    // |-------------|--------------------------|--------------------------|
    // | `0x00..=7F` | ASCII / single-byte UTF-8| can be true              |
    // | `0x80..=BF` | UTF-8 continuation byte  | always false             |
    // | `0xC2..=F4` | UTF-8 leading byte       | always false             |
    // Therefore ASCII byte-wise trimming cannot cut through a multibyte code point.
    while start_offset < len && bytes[start_offset].is_ascii_whitespace() {
        start_offset += 1;
    }
    while end_offset > start_offset && bytes[end_offset - 1].is_ascii_whitespace() {
        end_offset -= 1;
    }

    debug_assert!(content.is_char_boundary(body_start));
    debug_assert!(content.is_char_boundary(start_offset));
    debug_assert!(content.is_char_boundary(end_offset));

    let body_len = end_offset - start_offset;
    if start_offset == 0 && body_len == len {
        return content;
    }

    unsafe {
        let vec = content.as_mut_vec();
        core::ptr::copy(vec.as_ptr().add(start_offset), vec.as_mut_ptr(), body_len);
        vec.set_len(body_len);
    }

    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RawFrontmatter;
    use indoc::indoc;

    #[test]
    fn parse_extracts_frontmatter_and_content() {
        let input: &str = indoc! {"
            ---
            mode: subagent
            description: Test agent
            ---

            Prompt body here."
        };
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.data.description, "Test agent".to_string());
        assert_eq!(result.content, "Prompt body here.");
    }

    #[test]
    fn parse_trims_body_whitespace() {
        let input = indoc! {"
            ---
            mode: primary
            description: Test
            ---

              indented

            trailing
        "};
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "indented\n\ntrailing");
    }

    #[test]
    fn parse_trims_ascii_whitespace_with_multibyte_body() {
        let input = indoc! {"
            ---
            mode: primary
            description: Test
            ---

              🙂 café 漢字  
        "};
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "🙂 café 漢字");
    }

    #[test]
    fn parse_handles_empty_body() {
        let input = indoc! {"
            ---
            mode: primary
            description: Test
            ---"
        };
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert!(result.content.is_empty());
    }

    #[test]
    fn parse_handles_empty_frontmatter() {
        // FIX #2: Handle ---\n--- case (empty YAML)
        let input = indoc! {"
            ---
            description: Test
            ---
            body"
        };
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_handles_whitespace_only_frontmatter() {
        // FIX #2: Handle frontmatter with only whitespace
        let input = indoc! {"
            ---
            description: Test
            ---
            body"
        };
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body");
    }

    #[test]
    fn parse_trims_crlf_in_body() {
        // FIX #3: Body should normalize CRLF to LF
        let input = "---\nmode: subagent\ndescription: Test\n---\nline1\r\nline2\r\n";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "line1\nline2");
    }

    #[test]
    fn parse_trims_crlf_body_with_crlf_frontmatter() {
        // FIX #3: CRLF in frontmatter should normalize body
        let input = "---\r\nmode: subagent\r\ndescription: Test\r\n---\r\nbody\r\nline2\r\n";
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, "body\nline2");
    }

    #[test]
    fn parse_rejects_frontmatter_not_at_start() {
        let input = indoc! {"
            some text
            ---
            mode: subagent
            ---
            body"
        };
        let result: Result<AgentParseResult<RawFrontmatter>, AgentParseError> =
            parse_agent(input.to_string());

        assert!(matches!(result, Err(AgentParseError::MissingFrontmatter)));
    }

    #[test]
    fn parse_handles_bom() {
        let input = indoc! {"
            ---
            mode: subagent
            description: Test
            ---
            body"
        };
        let input = format!("\u{FEFF}{}", input);
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
        let input = indoc! {"
            ---
            [invalid yaml
            ---
            body"
        };
        let result: Result<AgentParseResult<RawFrontmatter>, AgentParseError> =
            parse_agent(input.to_string());

        assert!(matches!(result, Err(AgentParseError::InvalidYaml { .. })));
    }

    #[test]
    fn block_scalar_no_trailing_newline() {
        let input = indoc! {"
            ---
            model: provider/model:tag
            description: Test
            ---
            body"
        };
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
