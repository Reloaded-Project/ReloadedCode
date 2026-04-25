//! Provider-facing metadata for the `task` tool.

use super::ParamMetadata;

/// Canonical tool name.
pub const NAME: &str = "task";

/// Static description prefix before rendering available targets.
pub const DESCRIPTION_PREFIX: &str = "Delegate work to one of the listed subagents.";

/// Parameter metadata.
pub mod param {
    use super::ParamMetadata;

    /// `description` parameter metadata.
    pub const DESCRIPTION: ParamMetadata =
        ParamMetadata::new("description", "Short task label.", true);

    /// `prompt` parameter metadata.
    pub const PROMPT: ParamMetadata =
        ParamMetadata::new("prompt", "Full instructions for the delegated agent.", true);

    /// `subagent_type` parameter metadata.
    pub const SUBAGENT_TYPE: ParamMetadata =
        ParamMetadata::new("subagent_type", "Exact name of the target subagent.", true);

    /// `command` parameter metadata.
    pub const COMMAND: ParamMetadata =
        ParamMetadata::new("command", "Source command or slash-command context.", false);
}
