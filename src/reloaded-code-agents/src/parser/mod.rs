//! # Frontmatter Parser
//!
//! Parses markdown documents into:
//! - typed frontmatter (`T`)
//! - prompt body text
//!
//! ## Expected Input
//! ```text
//! ---
//! description: Example agent
//! ---
//! Prompt body here.
//! ```
//!
//! ## Normalization
//! - Converts CRLF to LF for the full document.
//! - Trims leading/trailing ASCII whitespace from the body.
//! - Preprocesses YAML before deserialization (see [`preprocessor`]).

mod preprocessor;

use crlf_to_lf_inplace::crlf_to_lf_inplace;
use preprocessor::preprocess_frontmatter_yaml;
use serde::de::DeserializeOwned;
use serde_yaml::Value;
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

    /// Schema validation failed.
    #[error("schema validation failed: {message}")]
    SchemaValidation {
        /// Validation error message.
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

    let yaml_value: Value = serde_yaml::from_str(yaml_preprocessed.as_ref()).map_err(|e| {
        AgentParseError::InvalidYaml {
            message: e.to_string(),
        }
    })?;
    validate_headless_compatibility(&yaml_value)?;

    let data: T =
        serde_yaml::from_value(yaml_value).map_err(|e| AgentParseError::SchemaValidation {
            message: e.to_string(),
        })?;

    // Extract body by mutating and reusing the existing allocation.
    let body = extract_body_inplace(content, offsets.body_start);

    Ok(AgentParseResult {
        data,
        content: body,
    })
}

/// Validates frontmatter is compatible with headless operation.
///
/// Rejects features requiring user interaction (e.g., "ask" permissions)
/// that are unsupported in non-interactive contexts.
fn validate_headless_compatibility(frontmatter: &Value) -> Result<(), AgentParseError> {
    // Skip if root isn't a mapping
    let Value::Mapping(root) = frontmatter else {
        return Ok(());
    };

    let permission_key = Value::String("permission".to_string());
    let task_key = Value::String("task".to_string());

    // Extract permission.task for validation
    //
    // ```yaml
    // permission:
    //   task: <action>              # e.g., "allow", "deny", or "ask"
    // ```
    //
    // or:
    //
    // ```yaml
    // permission:
    //   task:
    //     <pattern>: <action>       # e.g., "*": "ask"
    // ```
    //
    // See `PermissionRule` for the target type.
    let Some(Value::Mapping(permission_map)) = root.get(&permission_key) else {
        return Ok(());
    };
    let Some(task_rule) = permission_map.get(&task_key) else {
        return Ok(());
    };

    // Reject "ask" - requires interactive user confirmation
    if task_rule_contains_ask(task_rule) {
        return Err(AgentParseError::SchemaValidation {
            message: "permission.task: ask is unsupported; use allow or deny".to_string(),
        });
    }
    Ok(())
}

fn task_rule_contains_ask(rule: &Value) -> bool {
    match rule {
        // Scalar: `task: ask`
        Value::String(action) => action.eq_ignore_ascii_case("ask"),
        // Mapping: `task: "*": ask`
        Value::Mapping(patterns) => patterns.values().any(
            |value| matches!(value, Value::String(action) if action.eq_ignore_ascii_case("ask")),
        ),
        _ => false,
    }
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
    use crate::types::RawFrontmatter;
    use indoc::indoc;
    use rstest::rstest;

    /// Expected error variant for parameterized error-case tests.
    #[derive(Debug, Clone, Copy)]
    enum ExpectedParseError {
        MissingFrontmatter,
        InvalidYaml,
        SchemaValidation(&'static str),
    }

    #[rstest]
    #[case::extracts_frontmatter_and_content(
        indoc! {"
            ---
            mode: subagent
            description: Test agent
            ---

            Prompt body here."
        },
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "Prompt body here.", "Test agent", None, None
    )]
    #[case::trims_body_whitespace(
        indoc! {"
            ---
            mode: primary
            description: Test
            ---

              indented

            trailing
        "},
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "indented\n\ntrailing", "Test", None, None
    )]
    #[case::trims_ascii_whitespace_with_multibyte_body(
        indoc! {"
            ---
            mode: primary
            description: Test
            ---

              🙂 café 漢字  
        "},
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "🙂 café 漢字", "Test", None, None
    )]
    #[case::handles_empty_body(
        indoc! {"
            ---
            mode: primary
            description: Test
            ---"
        },
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "", "Test", None, None
    )]
    #[case::handles_bom(
        indoc! {"
            ---
            mode: subagent
            description: Test
            ---
            body"
        },
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        true, "body", "Test", None, None
    )]
    #[case::block_scalar_no_trailing_newline(
        indoc! {"
            ---
            model: provider/model:tag
            description: Test
            ---
            body"
        },
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "body", "Test", Some("provider/model:tag"), None
    )]
    #[case::accepts_permission_task_allow_scalar(
        indoc! {"
            ---
            description: Test
            permission:
              task: allow
            ---
            body"
        },
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "body", "Test", None, None
    )]
    #[case::accepts_hidden_true_no_validation_failure(
        indoc! {"
            ---
            description: Test
            hidden: true
            ---
            body"
        },
        // prepend_bom, expected_content, expected_description, expected_model, expected_hidden
        false, "body", "Test", None, Some(true)
    )]
    fn parse_agent_success_cases(
        #[case] input: &str,
        #[case] prepend_bom: bool,
        #[case] expected_content: &str,
        #[case] expected_description: &str,
        #[case] expected_model: Option<&str>,
        #[case] expected_hidden: Option<bool>,
    ) {
        let input = if prepend_bom {
            format!("\u{FEFF}{input}")
        } else {
            input.to_string()
        };
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input).unwrap();

        assert_eq!(result.content, expected_content);
        assert_eq!(&*result.data.description, expected_description);
        assert_eq!(result.data.model.as_deref(), expected_model);
        assert_eq!(result.data.hidden, expected_hidden.unwrap_or(false));
    }

    #[rstest]
    #[case::empty_frontmatter_schema_validation(
        indoc! {"
            ---
             
            ---
            body"
        },
        ExpectedParseError::SchemaValidation("missing field `description`")
    )]
    #[case::frontmatter_not_at_start_returns_missing_frontmatter(
        indoc! {"
            some text
            ---
            mode: subagent
            ---
            body"
        },
        ExpectedParseError::MissingFrontmatter
    )]
    #[case::missing_frontmatter_returns_missing_frontmatter(
        "No frontmatter here",
        ExpectedParseError::MissingFrontmatter
    )]
    #[case::invalid_yaml_returns_invalid_yaml(
        indoc! {"
            ---
            [invalid yaml
            ---
            body"
        },
        ExpectedParseError::InvalidYaml
    )]
    fn parse_agent_error_cases(#[case] input: &str, #[case] expected_error: ExpectedParseError) {
        let result = parse_agent::<RawFrontmatter>(input.to_string());

        match (result, expected_error) {
            (Err(AgentParseError::MissingFrontmatter), ExpectedParseError::MissingFrontmatter) => {}
            (Err(AgentParseError::InvalidYaml { .. }), ExpectedParseError::InvalidYaml) => {}
            (
                Err(AgentParseError::SchemaValidation { message }),
                ExpectedParseError::SchemaValidation(expected_fragment),
            ) => assert!(message.contains(expected_fragment)),
            (other, expected) => panic!("expected {expected:?}, got {other:?}"),
        }
    }

    // CRLF cases use inline string literals (not indoc!) because they need
    // explicit \r\n escape sequences that indoc! cannot represent.
    #[rstest]
    #[case::body_only_crlf_normalizes_to_lf(
        "---\nmode: subagent\ndescription: Test\n---\nline1\r\nline2\r\n",
        "line1\nline2"
    )]
    #[case::frontmatter_and_body_crlf_normalize_to_lf(
        "---\r\nmode: subagent\r\ndescription: Test\r\n---\r\nbody\r\nline2\r\n",
        "body\nline2"
    )]
    fn parse_agent_crlf_normalization(#[case] input: &str, #[case] expected_content: &str) {
        let result: AgentParseResult<RawFrontmatter> = parse_agent(input.to_string()).unwrap();

        assert_eq!(result.content, expected_content);
    }

    #[rstest]
    #[case::rejects_task_ask_scalar(
        indoc! {"
            ---
            description: Test
            permission:
              task: ask
            ---
            body"
        }
    )]
    #[case::rejects_task_ask_pattern_map(
        indoc! {"
            ---
            description: Test
            permission:
              task:
                '*': ask
            ---
            body"
        }
    )]
    fn parse_agent_permission_validation(#[case] input: &str) {
        let result = parse_agent::<RawFrontmatter>(input.to_string());

        assert!(matches!(
            result,
            Err(AgentParseError::SchemaValidation { message })
                if message.contains("permission.task: ask is unsupported")
        ));
    }

    #[rstest]
    #[case::missing_frontmatter(
        AgentParseError::MissingFrontmatter, // error: missing frontmatter variant
        "missing frontmatter"                // expected: display message
    )]
    #[case::invalid_yaml(
        AgentParseError::InvalidYaml { message: "bad".to_string() }, // error: yaml parse failure
        "invalid YAML frontmatter: bad"                              // expected: includes error message
    )]
    #[case::schema_validation(
        AgentParseError::SchemaValidation { message: "schema bad".to_string() }, // error: schema check failure
        "schema validation failed: schema bad"                                   // expected: includes error message
    )]
    fn parse_error_display_messages(
        #[case] error: AgentParseError,
        #[case] expected_message: &str,
    ) {
        assert_eq!(error.to_string(), expected_message);
    }
}
