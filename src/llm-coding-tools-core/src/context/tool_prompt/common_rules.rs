//! Writes the shared `Common Rules` section for built-in tools.
//!
//! These helpers add rules that apply to more than one tool. Each rule is only
//! included when the matching tools are present.

use const_format::formatcp;

use super::{push_line, write_tool_list, ToolPromptFacts};
use crate::tool_metadata::{bash, edit, glob, grep, read, write};

/// Writes the shared rules for the current built-in tools.
pub(super) fn write_common_rules(facts: ToolPromptFacts, output: &mut String) {
    append_allowed_path_rule(facts, output);
    append_bash_rule(facts, output);
    append_search_rule(facts, output);
    append_write_rule(facts, output);
    append_read_before_edit_rule(facts, output);
}

/// Adds the allowed-path rule when any tool is limited to allowed directories.
fn append_allowed_path_rule(facts: ToolPromptFacts, output: &mut String) {
    if facts.has_allowed_path_tool {
        push_line(
            output,
            "- Only listed allowed directories may be accessed; other paths are rejected.",
        );
    }
}

/// Adds the rule that prefers file tools over `bash`.
fn append_bash_rule(facts: ToolPromptFacts, output: &mut String) {
    if !facts.has_bash {
        return;
    }

    let mut tools = [""; 5];
    let mut len = 0;
    if facts.has_glob {
        tools[len] = glob::NAME;
        len += 1;
    }
    if facts.has_grep {
        tools[len] = grep::NAME;
        len += 1;
    }
    if facts.has_read {
        tools[len] = read::NAME;
        len += 1;
    }
    if facts.has_edit {
        tools[len] = edit::NAME;
        len += 1;
    }
    if facts.has_write {
        tools[len] = write::NAME;
        len += 1;
    }
    if len == 0 {
        return;
    }

    output.push_str("- Prefer ");
    write_tool_list(output, &tools[..len]);
    push_line(
        output,
        formatcp!(" over `{}` for ordinary file work.", bash::NAME),
    );
}

/// Adds the rule that separates file search, content search, and full reads.
fn append_search_rule(facts: ToolPromptFacts, output: &mut String) {
    match (facts.has_glob, facts.has_grep, facts.has_read) {
        (true, true, true) => push_line(
            output,
            formatcp!(
                "- Use `{}` for file-name search, `{}` for content search, and `{}` for full-file inspection.",
                glob::NAME,
                grep::NAME,
                read::NAME,
            ),
        ),
        (true, true, false) => push_line(
            output,
            formatcp!("- Use `{}` for file-name search and `{}` for content search.", glob::NAME, grep::NAME),
        ),
        (true, false, true) => push_line(
            output,
            formatcp!("- Use `{}` to find files and `{}` for full-file inspection.", glob::NAME, read::NAME),
        ),
        (false, true, true) => push_line(
            output,
            formatcp!("- Use `{}` for content search and `{}` for full-file inspection.", grep::NAME, read::NAME),
        ),
        _ => {}
    }
}

/// Adds the rule that points small changes to `edit` and rewrites to `write`.
fn append_write_rule(facts: ToolPromptFacts, output: &mut String) {
    if facts.has_edit && facts.has_write {
        push_line(
            output,
            formatcp!(
                "- Prefer `{}` for targeted changes and `{}` for new files or full rewrites.",
                edit::NAME,
                write::NAME
            ),
        );
    }
}

/// Adds the rule to read a file before editing or overwriting it.
fn append_read_before_edit_rule(facts: ToolPromptFacts, output: &mut String) {
    match (facts.has_read, facts.has_edit, facts.has_write, facts.read_line_numbers) {
        (true, true, true, true) => push_line(
            output,
            formatcp!(
                "- Read a file before `{}` or overwriting it with `{}`; for `{}`, copy exact text from `{}` and omit any `{}` prefixes.",
                edit::NAME,
                write::NAME,
                edit::NAME,
                read::NAME,
                read::LINE_PREFIX_DISPLAY,
            ),
        ),
        (true, true, true, false) => push_line(
            output,
            formatcp!(
                "- Read a file before `{}` or overwriting it with `{}`; for `{}`, copy exact text from `{}`.",
                edit::NAME,
                write::NAME,
                edit::NAME,
                read::NAME,
            ),
        ),
        (true, true, false, true) => push_line(
            output,
            formatcp!(
                "- Read a file before `{}`, then copy exact text from `{}` and omit any `{}` prefixes.",
                edit::NAME,
                read::NAME,
                read::LINE_PREFIX_DISPLAY,
            ),
        ),
        (true, true, false, false) => push_line(
            output,
            formatcp!(
                "- Read a file before `{}`, then copy exact text from `{}`.",
                edit::NAME,
                read::NAME,
            ),
        ),
        (true, false, true, _) => {
            push_line(output, formatcp!("- Read a file before overwriting it with `{}`.", write::NAME))
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{COMMON_RULES_HEADER, COMMON_RULES_SECTION_MAX_SIZE};

    #[test]
    fn common_rules_max_size_matches_all_rendered_variants() {
        let mut max_len = 0;

        for has_allowed_path_tool in [false, true] {
            for has_bash in [false, true] {
                for has_read in [false, true] {
                    for read_line_numbers in [false, true] {
                        for has_write in [false, true] {
                            for has_edit in [false, true] {
                                for has_glob in [false, true] {
                                    for has_grep in [false, true] {
                                        let facts = ToolPromptFacts {
                                            has_allowed_path_tool,
                                            has_bash,
                                            has_read,
                                            read_line_numbers,
                                            has_write,
                                            has_edit,
                                            has_glob,
                                            has_grep,
                                        };
                                        if !facts.has_common_rules() {
                                            continue;
                                        }

                                        let mut rendered = String::from(COMMON_RULES_HEADER);
                                        write_common_rules(facts, &mut rendered);
                                        max_len = max_len.max(rendered.len());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        assert_eq!(max_len, COMMON_RULES_SECTION_MAX_SIZE);
    }
}
