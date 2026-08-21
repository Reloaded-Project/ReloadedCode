//! Vendor-history projection onto neutral compaction entries.
//!
//! `Vec<ModelRequest>` parts become [`CompactEntry`] views, each
//! carrying its native part as the preserved payload; a compacted
//! history rebuilds into requests, reusing untouched parts and
//! rebuilding injected entries from the view.
//!
//! Next: see the parent module for detection, publication, and the
//! model wrapper driving this.

use crate::agent_runtime::stream_events::user_content_text_ref;
use reloaded_code_core::CompactEntry;
use reloaded_code_core::hooks::RunMessageRole;
use serdes_ai::core::messages::{RetryContent, ToolCallArgs, ToolReturnContent};
use serdes_ai::core::{
    ModelRequest, ModelRequestPart, ModelResponse, ModelResponsePart, SystemPromptPart,
    UserContent, UserPromptPart,
};

/// Projects the vendor history onto neutral entries, carrying each
/// native part as the preserved payload.
pub(super) fn project_history(messages: &[ModelRequest]) -> Vec<CompactEntry> {
    let part_count = messages.iter().map(|message| message.parts.len()).sum();
    let mut entries = Vec::with_capacity(part_count);
    for message in messages {
        for part in &message.parts {
            let (role, text) = part_role_text(part);
            entries.push(CompactEntry::new_preserved(
                role,
                text,
                Box::new(part.clone()),
            ));
        }
    }
    entries
}

/// Applies a compacted history: preserved entries reuse their native
/// parts, injected entries rebuild from the structured view.
pub(super) fn rebuild_history(entries: Vec<CompactEntry>) -> Vec<ModelRequest> {
    entries
        .into_iter()
        .map(|mut entry| {
            let part = match entry.take_preserved() {
                Some(preserved) => match preserved.downcast::<ModelRequestPart>() {
                    Ok(part) => *part,
                    Err(_) => rebuilt_part(entry.role(), entry.text()),
                },
                None => rebuilt_part(entry.role(), entry.text()),
            };
            ModelRequest::with_parts(vec![part])
        })
        .collect()
}

/// Role and view text of one history part.
fn part_role_text(part: &ModelRequestPart) -> (RunMessageRole, String) {
    match part {
        ModelRequestPart::SystemPrompt(part) => (RunMessageRole::System, part.content.clone()),
        ModelRequestPart::UserPrompt(part) => {
            (RunMessageRole::User, user_content_text_ref(&part.content))
        }
        ModelRequestPart::RetryPrompt(part) => (
            RunMessageRole::User,
            match &part.content {
                RetryContent::Text(text) => text.clone(),
                other => other.message().to_owned(),
            },
        ),
        ModelRequestPart::ToolReturn(part) => (
            RunMessageRole::Tool,
            match &part.content {
                ToolReturnContent::Text { content } => content.clone(),
                other => other.to_string_content(),
            },
        ),
        ModelRequestPart::BuiltinToolReturn(part) => (
            RunMessageRole::Tool,
            serde_json::to_string(&part.content).unwrap_or_else(|_| format!("{part:?}")),
        ),
        ModelRequestPart::ModelResponse(response) => {
            (RunMessageRole::Assistant, response_text(response))
        }
    }
}

/// Rebuilds a native part from the structured view.
///
/// A tool entry rebuilds as a user-role note: the view carries no
/// call id, and an id-less tool result could be rejected by the
/// provider.
fn rebuilt_part(role: RunMessageRole, text: &str) -> ModelRequestPart {
    match role {
        RunMessageRole::System => ModelRequestPart::SystemPrompt(SystemPromptPart::new(text)),
        RunMessageRole::User => {
            ModelRequestPart::UserPrompt(UserPromptPart::new(UserContent::Text(text.to_owned())))
        }
        RunMessageRole::Assistant => {
            ModelRequestPart::ModelResponse(Box::new(ModelResponse::text(text)))
        }
        RunMessageRole::Tool => ModelRequestPart::UserPrompt(UserPromptPart::new(
            UserContent::Text(format!("[tool result] {text}")),
        )),
    }
}

/// View text of one model response: text parts joined, with tool
/// calls rendered inline so call-only turns stay visible.
fn response_text(response: &ModelResponse) -> String {
    let mut text_len = 0usize;
    let mut tool_calls = 0usize;
    for part in &response.parts {
        match part {
            ModelResponsePart::Text(text) => text_len += text.content.len(),
            ModelResponsePart::ToolCall(_) => tool_calls += 1,
            _ => {}
        }
    }
    // Per tool call: newline, name, arguments, and brackets.
    let mut text = String::with_capacity(text_len + tool_calls * 64);
    for part in &response.parts {
        match part {
            ModelResponsePart::Text(text_part) => text.push_str(&text_part.content),
            ModelResponsePart::ToolCall(call) => {
                let args = match &call.args {
                    ToolCallArgs::String(raw) => raw.clone(),
                    other => other.to_json_string().unwrap_or_default(),
                };
                text.push_str(&format!("\n[tool call {}({args})]", call.tool_name));
            }
            _ => {}
        }
    }
    text
}
