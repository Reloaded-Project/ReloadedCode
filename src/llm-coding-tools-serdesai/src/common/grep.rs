//! Shared helpers for Grep tool implementations.

use llm_coding_tools_core::tools::GrepOutput;
use serde_json::json;
use serdes_ai::tools::ToolReturn;

const NO_MATCHES_FOUND: &str = "No matches found.";

#[inline]
pub(crate) fn output_to_return<const LINE_NUMBERS: bool>(
    output: GrepOutput,
    limit: usize,
    max_line_len: usize,
) -> ToolReturn {
    if output.partial {
        let content = output.format::<LINE_NUMBERS>(limit, max_line_len);
        return ToolReturn::json(json!({
            "content": content,
            "partial": true,
            "errors": output.errors,
            "match_count": output.match_count,
            "truncated": output.truncated,
        }));
    }

    if output.files.is_empty() {
        return ToolReturn::text(NO_MATCHES_FOUND);
    }

    ToolReturn::text(output.format::<LINE_NUMBERS>(limit, max_line_len))
}
