//! Example-only mock tools used to build prompt previews.

use reloaded_code_core::context::{PathMode, ToolContext, ToolPrompt};
use reloaded_code_core::{tool_metadata, SystemPromptBuilder};

use super::{GrepConfig, PromptCase, ReadConfig};

/// Registers the mock tools needed for one prompt example case.
pub(super) fn track_case_tools(builder: &mut SystemPromptBuilder, case: PromptCase) {
    if let Some(read) = case.read {
        track_read(builder, read);
    }
    if let Some(write) = case.write {
        track_write(builder, write);
    }
    if let Some(edit) = case.edit {
        track_edit(builder, edit);
    }
    if case.bash {
        let _ = builder.track(MockBashTool);
    }
    if let Some(glob) = case.glob {
        track_glob(builder, glob);
    }
    if let Some(grep) = case.grep {
        track_grep(builder, grep);
    }
    if case.webfetch {
        let _ = builder.track(MockWebFetchTool);
    }
    if case.todo_write {
        let _ = builder.track(MockTodoWriteTool);
    }
    if case.todo_read {
        let _ = builder.track(MockTodoReadTool);
    }
    if !case.task_targets.is_empty() {
        let _ = builder.track(MockTaskTool);
    }
}

fn track_read(builder: &mut SystemPromptBuilder, config: ReadConfig) {
    match (config.path_mode, config.line_numbers) {
        (PathMode::Absolute, true) => {
            let _ = builder.track(MockReadTool::<false, true>);
        }
        (PathMode::Absolute, false) => {
            let _ = builder.track(MockReadTool::<false, false>);
        }
        (PathMode::Allowed, true) => {
            let _ = builder.track(MockReadTool::<true, true>);
        }
        (PathMode::Allowed, false) => {
            let _ = builder.track(MockReadTool::<true, false>);
        }
    }
}

fn track_write(builder: &mut SystemPromptBuilder, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => {
            let _ = builder.track(MockWriteTool::<false>);
        }
        PathMode::Allowed => {
            let _ = builder.track(MockWriteTool::<true>);
        }
    }
}

fn track_edit(builder: &mut SystemPromptBuilder, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => {
            let _ = builder.track(MockEditTool::<false>);
        }
        PathMode::Allowed => {
            let _ = builder.track(MockEditTool::<true>);
        }
    }
}

fn track_glob(builder: &mut SystemPromptBuilder, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => {
            let _ = builder.track(MockGlobTool::<false>);
        }
        PathMode::Allowed => {
            let _ = builder.track(MockGlobTool::<true>);
        }
    }
}

fn track_grep(builder: &mut SystemPromptBuilder, config: GrepConfig) {
    match (config.path_mode, config.line_numbers) {
        (PathMode::Absolute, true) => {
            let _ = builder.track(MockGrepTool::<false, true>);
        }
        (PathMode::Absolute, false) => {
            let _ = builder.track(MockGrepTool::<false, false>);
        }
        (PathMode::Allowed, true) => {
            let _ = builder.track(MockGrepTool::<true, true>);
        }
        (PathMode::Allowed, false) => {
            let _ = builder.track(MockGrepTool::<true, false>);
        }
    }
}

const fn path_mode<const ALLOWED: bool>() -> PathMode {
    if ALLOWED {
        PathMode::Allowed
    } else {
        PathMode::Absolute
    }
}

macro_rules! path_tool_with_line_numbers {
    ($tool:ident, $name:path, $variant:ident) => {
        struct $tool<const ALLOWED: bool, const LINE_NUMBERS: bool>;

        impl<const ALLOWED: bool, const LINE_NUMBERS: bool> ToolContext
            for $tool<ALLOWED, LINE_NUMBERS>
        {
            fn name(&self) -> &'static str {
                $name
            }

            fn context(&self) -> ToolPrompt {
                ToolPrompt::$variant {
                    path_mode: path_mode::<ALLOWED>(),
                    line_numbers: LINE_NUMBERS,
                }
            }
        }
    };
}

macro_rules! path_tool {
    ($tool:ident, $name:path, $variant:ident) => {
        struct $tool<const ALLOWED: bool>;

        impl<const ALLOWED: bool> ToolContext for $tool<ALLOWED> {
            fn name(&self) -> &'static str {
                $name
            }

            fn context(&self) -> ToolPrompt {
                ToolPrompt::$variant {
                    path_mode: path_mode::<ALLOWED>(),
                }
            }
        }
    };
}

path_tool_with_line_numbers!(MockReadTool, tool_metadata::read::NAME, Read);
path_tool!(MockWriteTool, tool_metadata::write::NAME, Write);
path_tool!(MockEditTool, tool_metadata::edit::NAME, Edit);
path_tool!(MockGlobTool, tool_metadata::glob::NAME, Glob);
path_tool_with_line_numbers!(MockGrepTool, tool_metadata::grep::NAME, Grep);

struct MockBashTool;

impl ToolContext for MockBashTool {
    fn name(&self) -> &'static str {
        tool_metadata::bash::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Bash {
            network_disabled: false,
            sandboxed: false,
        }
    }
}

struct MockWebFetchTool;

impl ToolContext for MockWebFetchTool {
    fn name(&self) -> &'static str {
        tool_metadata::webfetch::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::WebFetch
    }
}

struct MockTodoWriteTool;

impl ToolContext for MockTodoWriteTool {
    fn name(&self) -> &'static str {
        tool_metadata::todo_write::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::TodoWrite
    }
}

struct MockTodoReadTool;

impl ToolContext for MockTodoReadTool {
    fn name(&self) -> &'static str {
        tool_metadata::todo_read::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::TodoRead
    }
}

struct MockTaskTool;

impl ToolContext for MockTaskTool {
    fn name(&self) -> &'static str {
        tool_metadata::task::NAME
    }

    fn context(&self) -> ToolPrompt {
        ToolPrompt::Task
    }
}
