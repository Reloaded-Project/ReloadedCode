//! Writes the section for each built-in tool.
//!
//! Each helper in this module writes one tool's guidance text, which keeps the
//! top-level renderer small and easy to follow.

use const_format::formatcp;

use super::{push_block, push_line, write_tool_list, PathMode, ToolPrompt, ToolPromptFacts};
use crate::tool_metadata::{bash, edit, glob, grep, read, webfetch};

pub(super) fn render_tool(prompt: ToolPrompt, output: &mut String, facts: ToolPromptFacts) {
    match prompt {
        ToolPrompt::Static(text) => output.push_str(text),
        ToolPrompt::Bash => write_bash_section(output),
        ToolPrompt::Read {
            path_mode,
            line_numbers,
        } => write_read_section(output, facts, path_mode, line_numbers),
        ToolPrompt::Write { path_mode } => write_write_section(output, facts, path_mode),
        ToolPrompt::Edit { path_mode } => write_edit_section(output, facts, path_mode),
        ToolPrompt::Glob { path_mode } => write_glob_section(output, facts, path_mode),
        ToolPrompt::Grep {
            path_mode,
            line_numbers: _,
        } => write_grep_section(output, facts, path_mode),
        ToolPrompt::WebFetch => write_webfetch_section(output),
        ToolPrompt::TodoRead => write_todo_read_section(output),
        ToolPrompt::TodoWrite => write_todo_write_section(output),
        ToolPrompt::Task => write_task_section(output, facts),
    }
}

fn write_bash_section(output: &mut String) {
    push_block(
        output,
        formatcp!(
            "Runs one shell command in a fresh shell process.\n\
            - Use it for terminal work (git, package managers, test runners, docker) and shell-native search/filter jobs the specialized tools do not handle well.\n\
            - Output includes stdout, stderr under `[stderr]`, and non-zero exit codes as `[exit code: N]`.\n\
            - For independent commands, make parallel `{}` calls. For dependent commands, use one call with `&&`.\n\
            - Quote paths that contain spaces.\n",
            bash::NAME,
        ),
    );
}

fn write_read_section(
    output: &mut String,
    facts: ToolPromptFacts,
    path_mode: PathMode,
    line_numbers: bool,
) {
    match path_mode {
        PathMode::Absolute => push_line(
            output,
            "Reads a file from an absolute path on the local filesystem.",
        ),
        PathMode::Allowed => push_line(output, "Reads a file in allowed directories."),
    }

    if line_numbers {
        push_line(
            output,
            formatcp!(
                "- Returns `{}` text. Lines over `{}` chars are truncated.",
                read::LINE_PREFIX_DISPLAY,
                read::MAX_LINE_LENGTH,
            ),
        );
    } else {
        push_line(
            output,
            formatcp!(
                "- Returns raw text. Lines over `{}` chars are truncated.",
                read::MAX_LINE_LENGTH,
            ),
        );
    }

    match (facts.has_glob, facts.has_bash) {
        (true, true) => push_line(
            output,
            formatcp!(
                "- Reads files, not directories. Use `{}` to find files or `{}` for directory listings.",
                glob::NAME,
                bash::NAME,
            ),
        ),
        (true, false) => {
            push_line(
                output,
                formatcp!("- Reads files, not directories. Use `{}` to find files.", glob::NAME),
            )
        }
        (false, true) => {
            push_line(
                output,
                formatcp!("- Reads files, not directories. Use `{}` for directory listings.", bash::NAME),
            )
        }
        (false, false) => push_line(output, "- Reads files, not directories."),
    }

    push_block(
        output,
        "- Missing files return an error. Non-text files are returned as text bytes; there is no special image rendering.\n\
- Read related files in parallel when useful.\n",
    );
}

fn write_write_section(output: &mut String, facts: ToolPromptFacts, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => push_line(
            output,
            "Writes a file to an absolute path and creates parent directories if needed.",
        ),
        PathMode::Allowed => push_line(
            output,
            "Writes a file in allowed directories and creates parent directories if needed.",
        ),
    }

    push_line(output, "- Existing files are overwritten.");
    if !facts.has_edit {
        push_line(
            output,
            "- Use this for new files or full rewrites, not small edits.",
        );
    }
}

fn write_edit_section(output: &mut String, facts: ToolPromptFacts, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => push_line(
            output,
            "Performs exact string replacement in a file at an absolute path.",
        ),
        PathMode::Allowed => push_line(
            output,
            "Performs exact string replacement in a file in allowed directories.",
        ),
    }

    if !facts.has_read {
        push_line(
            output,
            formatcp!(
                "- `{}` must match the existing file text exactly.",
                edit::param::OLD_STRING.name
            ),
        );
    }
    push_block(
        output,
        formatcp!(
            "- Without `{}` the edit fails if `{}` is missing or appears more than once.\n\
            - The edit also fails if `{}` is empty or equal to `{}`.\n",
            edit::param::REPLACE_ALL.name,
            edit::param::OLD_STRING.name,
            edit::param::OLD_STRING.name,
            edit::param::NEW_STRING.name,
        ),
    );
}

fn write_glob_section(output: &mut String, facts: ToolPromptFacts, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => push_line(
            output,
            "Find files by glob pattern from an absolute directory path.",
        ),
        PathMode::Allowed => {
            push_line(output, "Find files by glob pattern in allowed directories.")
        }
    }

    push_block(
        output,
        formatcp!(
            "- Supports `*`, `**`, `?`, `[abc]`, and `{{a,b}}`.\n\
            - Returns matching file paths relative to `{}`.\n\
            - Results are capped at `{}`; large result sets are returned with `truncated: true`.\n",
            glob::param::PATH_ABSOLUTE.name,
            glob::MAX_RESULTS,
        ),
    );
    if !facts.has_grep {
        push_line(output, "- Use it for file-name search, not content search.");
    }
}

fn write_grep_section(output: &mut String, facts: ToolPromptFacts, path_mode: PathMode) {
    match path_mode {
        PathMode::Absolute => push_line(
            output,
            "Search file contents with a regex from an absolute directory path.",
        ),
        PathMode::Allowed => push_line(
            output,
            "Search file contents with a regex in allowed directories.",
        ),
    }

    push_block(
        output,
        formatcp!(
            "- `{}` must not be empty. Search is single-line only; there is no multiline matching.\n\
            - Returns matches grouped by file.\n",
            grep::param::PATTERN.name,
        ),
    );
    if facts.has_bash {
        push_line(output, "- Use this instead of shell `grep`/`rg`.");
    }
    if !facts.has_glob && !facts.has_read {
        push_line(
            output,
            "- Use it for content search, not file-name search or full-file inspection.",
        );
    }
}

fn write_webfetch_section(output: &mut String) {
    push_block(
        output,
        formatcp!(
            "Fetches one URL.\n\
            - Output starts with `[content-type - bytes]`.\n\
            - Maximum response size is `{}` MiB.\n\
            - Use this for known URLs, not web search. Prefer a more specialized web tool when one exists.\n",
            webfetch::MAX_RESPONSE_SIZE_MIB,
        ),
    );
}

fn write_todo_read_section(output: &mut String) {
    push_block(
        output,
        "Reads the current todo list.\n\
- Output is plain text: either `No tasks.` or one line per task with status icon, priority, id, and content.\n\
- Use it before starting or resuming complex work when you need the current task list.\n",
    );
}

fn write_todo_write_section(output: &mut String) {
    push_block(
        output,
        formatcp!(
            "Replaces the session todo list.\n\
            - Use it for multi-step or non-trivial work, or when the user asks for task tracking. Skip it for a single small task.\n\
            - Send the full desired list each time; this tool replaces the whole list.\n\
            - `{}` and `{}` must not be empty.\n\
            - Keep task text short and imperative. Update statuses as you work; keep one `in_progress` task when practical.\n",
            crate::tool_metadata::todo_write::param::ID.name,
            crate::tool_metadata::todo_write::param::CONTENT.name,
        ),
    );
}

fn write_task_section(output: &mut String, facts: ToolPromptFacts) {
    push_block(
        output,
        formatcp!(
            "Delegate a focused job to another agent.\n\
            - This runtime is stateless. Do not pass `{}`.\n\
            - Use it for real delegation, custom slash commands, or independent sub-work you can run in parallel.\n",
            crate::tool_metadata::task::param::SESSION_ID.name,
        ),
    );

    let mut tools = [""; 3];
    let mut len = 0;
    if facts.has_read {
        tools[len] = read::NAME;
        len += 1;
    }
    if facts.has_glob {
        tools[len] = glob::NAME;
        len += 1;
    }
    if facts.has_grep {
        tools[len] = grep::NAME;
        len += 1;
    }
    if len > 0 {
        output.push_str("- Do not use it when ");
        write_tool_list(output, &tools[..len]);
        push_line(output, " on one or a few files is enough.");
    }

    push_line(
        output,
        "- The delegated result is returned only to you, so summarize it for the user.",
    );
}
