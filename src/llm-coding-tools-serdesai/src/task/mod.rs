//! Adapter-facing Task helpers and runtime glue for SerdesAI.
//!
//! This module provides crate-private helpers for building and managing Task tools.
//!
//! - `task_tool_definition` - Builds the Task tool definition and schema (re-exported for internal use).
//! - `render_task_targets` - Renders callable targets for Task descriptions.
//!
//! The concrete runtime pieces that execute delegated work stay crate-private so
//! external callers use the adapter's public API instead of constructing Task
//! tools by hand.

mod definition;
mod handle;
mod tool;

pub(crate) use definition::task_tool_definition;
pub(crate) use handle::TaskHandle;
pub(crate) use tool::TaskTool;
