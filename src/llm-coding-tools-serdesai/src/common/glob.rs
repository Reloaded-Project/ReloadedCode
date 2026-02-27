//! Shared helpers for Glob tool implementations.

use llm_coding_tools_core::tools::GlobOutput;
use serde_json::json;
use serdes_ai::tools::ToolReturn;

const NO_FILES_FOUND: &str = "No files found matching the pattern.";

#[inline]
fn output_content(files: &[String]) -> String {
    if files.is_empty() {
        NO_FILES_FOUND.to_string()
    } else {
        files.join("\n")
    }
}

#[inline]
pub(crate) fn output_to_return(output: GlobOutput) -> ToolReturn {
    let content = output_content(&output.files);

    if output.partial {
        return ToolReturn::json(json!({
            "content": content,
            "partial": true,
            "errors": output.errors,
            "truncated": output.truncated,
        }));
    }

    if output.truncated {
        ToolReturn::json(json!({
            "content": content,
            "truncated": true,
        }))
    } else {
        ToolReturn::text(content)
    }
}
