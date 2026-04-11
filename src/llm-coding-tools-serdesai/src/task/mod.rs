//! Adapter-facing Task helpers and runtime glue for SerdesAI.
//!
//! # Public API
//! - [`task_tool_definition`] - Builds the Task tool definition and schema.
//! - [`render_task_targets`] - Renders callable targets for Task descriptions.
//!
//! The concrete runtime pieces that execute delegated work stay crate-private so
//! callers use the public build API instead of constructing Task
//! tools by hand.

mod definition;
mod handle;
mod tool;

pub(crate) use definition::task_tool_definition;
pub(crate) use handle::TaskHandle;
pub(crate) use tool::TaskTool;
