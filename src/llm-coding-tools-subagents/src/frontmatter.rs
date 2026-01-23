//! Frontmatter parsing for markdown files with YAML headers.

use crate::error::{AgentConfigError, AgentConfigResult};
use serde::de::DeserializeOwned;
use std::path::Path;

/// Result of parsing a markdown file with frontmatter.
#[derive(Debug, Clone)]
pub struct FrontmatterParseResult<T> {
    /// Parsed frontmatter data.
    pub data: T,
    /// Markdown content after frontmatter (raw, not trimmed).
    pub content: String,
}

/// Preprocesses YAML frontmatter to handle inline `key: value:with:colons`.
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
/// ---
/// model: provider/model:tag
/// api_url: http://localhost:8080
/// ---
///
/// Output:
/// ---
/// model: |-
///   provider/model:tag
/// api_url: |-
///   http://localhost:8080
/// ---
/// ```
///
/// **Preserved unchanged** (already safe for YAML parsing):
///
/// ```text
/// Input:
/// ---
/// # comment: with:colon           # Comments are ignored
/// description: No colons here     # No colon in value
/// model: "provider/model:tag"     # Double-quoted
/// model: 'provider/model:tag'     # Single-quoted
/// content: |                      # Block scalar indicator
///   line:with:colon
/// items: ["a:b", "c:d"]           # Flow array syntax
/// config: { "key": "a:b" }        # Flow mapping syntax
/// ---
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
pub fn preprocess_frontmatter(content: &str) -> String {
    // Normalize CRLF to LF for consistent processing
    let content = content.replace("\r\n", "\n");

    // Frontmatter must start at position 0 (possibly after BOM)
    let start = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    if !start.starts_with("---") {
        return content;
    }

    let after_opener = if content.starts_with('\u{FEFF}') {
        4
    } else {
        3
    };

    // Find closing --- (must be on its own line, search AFTER the opening ---)
    // This handles empty frontmatter (---\n---) correctly
    let Some(end_offset) = content[after_opener..].find("\n---") else {
        return content;
    };
    let yaml_end = after_opener + end_offset;

    // Handle empty frontmatter (---\n---)
    if yaml_end == after_opener || content[after_opener..yaml_end].trim().is_empty() {
        return content;
    }

    let frontmatter = &content[after_opener..yaml_end];
    let mut result = Vec::with_capacity(frontmatter.lines().count());

    for line in frontmatter.lines() {
        let trimmed = line.trim();

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            result.push(line.to_string());
            continue;
        }

        // FIX #1: Skip continuation lines (indented) - explicit char checks instead of predicate
        if line.starts_with(' ') || line.starts_with('\t') {
            result.push(line.to_string());
            continue;
        }

        // Match key: value pattern
        let Some(colon_pos) = line.find(':') else {
            result.push(line.to_string());
            continue;
        };

        // Trim whitespace from key (handles "key : value" pattern)
        let key = line[..colon_pos].trim();

        // Validate key is identifier-like (starts with letter/underscore, contains only alphanumeric/underscore/hyphen)
        if !key
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            result.push(line.to_string());
            continue;
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            result.push(line.to_string());
            continue;
        }

        let value = line[colon_pos + 1..].trim();

        // Skip if value is empty, already quoted, or uses block scalar
        if value.is_empty()
            || value == ">"
            || value == "|"
            || value == "|-"
            || value == ">-"
            || value.starts_with('"')
            || value.starts_with('\'')
        {
            result.push(line.to_string());
            continue;
        }

        // Skip YAML flow syntax (maps/arrays) - don't corrupt { } or [ ]
        if value.starts_with('{') || value.starts_with('[') {
            result.push(line.to_string());
            continue;
        }

        // If value contains a colon, convert to block scalar with strip chomp
        // Use |- instead of | to avoid trailing newlines
        if value.contains(':') {
            result.push(format!("{key}: |-"));
            result.push(format!("  {value}"));
            continue;
        }

        result.push(line.to_string());
    }

    let processed = result.join("\n");

    // Replace frontmatter in original content
    let mut output = String::with_capacity(content.len() + 32);
    output.push_str(&content[..after_opener]);
    output.push_str(&processed);
    output.push_str(&content[yaml_end..]);
    output
}

/// Parses a markdown file with YAML frontmatter.
///
/// The file must start with `---` (at position 0, optionally after BOM),
/// followed by YAML, followed by `---` on its own line.
/// Content after the closing `---` is the markdown body (preserved exactly).
///
/// # Errors
///
/// Returns [`AgentConfigError::MissingFrontmatter`] if no valid frontmatter found.
/// Returns [`AgentConfigError::InvalidYaml`] if YAML parsing fails.
pub fn parse_frontmatter<T: DeserializeOwned>(
    content: &str,
    path: &Path,
) -> AgentConfigResult<FrontmatterParseResult<T>> {
    // FIX #3: Work with original content for body extraction, only normalize YAML slice

    // Frontmatter must start at position 0 (possibly after BOM)
    let start = content.strip_prefix('\u{FEFF}').unwrap_or(content);
    if !start.starts_with("---") {
        return Err(AgentConfigError::MissingFrontmatter {
            path: path.to_path_buf(),
        });
    }

    let has_bom = content.starts_with('\u{FEFF}');
    let after_opener = if has_bom { 4 } else { 3 };

    // FIX #2: Find closing --- by searching for "\n---" AFTER the opening "---"
    // This handles empty frontmatter (---\n---) because the search starts after "---"
    // and finds the "\n---" that follows immediately
    let Some(end_offset) = content[after_opener..].find("\n---") else {
        return Err(AgentConfigError::MissingFrontmatter {
            path: path.to_path_buf(),
        });
    };
    let yaml_end = after_opener + end_offset;

    // Skip the newline after opening --- to get yaml_start
    let yaml_start = content[after_opener..]
        .find('\n')
        .map(|n| after_opener + n + 1)
        .unwrap_or(after_opener);

    // Extract YAML slice (may be empty for ---\n---)
    let yaml_str = if yaml_start <= yaml_end {
        &content[yaml_start..yaml_end]
    } else {
        ""
    };

    // Normalize YAML slice only for parsing (handles CRLF in frontmatter)
    let yaml_normalized = yaml_str.replace("\r\n", "\n");

    // Preprocess to handle colons in values
    let yaml_preprocessed = if yaml_normalized.is_empty() {
        yaml_normalized
    } else {
        // Build a fake frontmatter document for preprocessing, then extract result
        let fake_doc = format!("---\n{}\n---\n", yaml_normalized);
        let processed = preprocess_frontmatter(&fake_doc);
        // Extract the YAML between the delimiters
        processed
            .strip_prefix("---\n")
            .and_then(|s| s.strip_suffix("\n---\n"))
            .unwrap_or(&yaml_normalized)
            .to_string()
    };

    // Find start of body content in ORIGINAL: after closing "---" and its trailing newline
    let closing_start = yaml_end + 1; // Position of \n before closing ---
    let after_closing = closing_start + 3; // Position after closing ---

    // FIX #3: Compute body start from ORIGINAL content, skip only the single newline after ---
    let content_start = if content[after_closing..].starts_with("\r\n") {
        after_closing + 2
    } else if content[after_closing..].starts_with('\n') {
        after_closing + 1
    } else {
        after_closing
    };

    let data: T =
        serde_yaml::from_str(&yaml_preprocessed).map_err(|e| AgentConfigError::InvalidYaml {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    // FIX #3: Return body from ORIGINAL content (preserves CRLF if present)
    let body = if content_start < content.len() {
        content[content_start..].to_string()
    } else {
        String::new()
    };

    Ok(FrontmatterParseResult {
        data,
        content: body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RawFrontmatter;

    #[test]
    fn preprocess_handles_colons_in_value() {
        let input = "---\nmodel: provider/model:tag\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("model: |-"));
        assert!(output.contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_preserves_quoted_values() {
        let input = "---\nmodel: \"provider/model:tag\"\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("model: \"provider/model:tag\""));
    }

    #[test]
    fn preprocess_preserves_block_scalars() {
        let input = "---\ndesc: |\n  multiline\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert_eq!(input, output);
    }

    #[test]
    fn preprocess_skips_comments() {
        let input = "---\n# comment: with:colon\nmode: subagent\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("# comment: with:colon"));
    }

    #[test]
    fn preprocess_skips_flow_mappings() {
        let input = "---\ntask: { \"*\": \"deny\" }\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("task: { \"*\": \"deny\" }"));
    }

    #[test]
    fn preprocess_skips_flow_arrays() {
        let input = "---\nitems: [\"a:b\", \"c:d\"]\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("items: [\"a:b\", \"c:d\"]"));
    }

    #[test]
    fn preprocess_handles_key_with_whitespace_around_colon() {
        let input = "---\nmodel : provider/model:tag\n---\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("model: |-"));
        assert!(output.contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_handles_crlf_line_endings() {
        let input = "---\r\nmodel: provider/model:tag\r\n---\r\nbody";
        let output = preprocess_frontmatter(input);
        assert!(output.contains("model: |-"));
        assert!(output.contains("  provider/model:tag"));
    }

    #[test]
    fn preprocess_skips_indented_lines() {
        // FIX #1: Indented lines should be skipped (continuation of previous value)
        let input = "---\ndesc: |\n  line:with:colons\n---\nbody";
        let output = preprocess_frontmatter(input);
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
        // Body preserves leading blank line
        assert_eq!(result.content, "\nPrompt body here.");
    }

    #[test]
    fn parse_preserves_body_whitespace() {
        let input = "---\nmode: primary\n---\n\n  indented\n\ntrailing\n";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "\n  indented\n\ntrailing\n");
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
    fn parse_preserves_crlf_in_body() {
        // FIX #3: Body should preserve CRLF line endings exactly
        let input = "---\nmode: subagent\n---\nline1\r\nline2\r\n";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "line1\r\nline2\r\n");
    }

    #[test]
    fn parse_preserves_crlf_body_with_crlf_frontmatter() {
        // FIX #3: CRLF in frontmatter should not affect body preservation
        let input = "---\r\nmode: subagent\r\n---\r\nbody\r\nline2\r\n";
        let result: FrontmatterParseResult<RawFrontmatter> =
            parse_frontmatter(input, Path::new("test.md")).unwrap();

        assert_eq!(result.content, "body\r\nline2\r\n");
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
