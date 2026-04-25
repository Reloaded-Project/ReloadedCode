use super::{sort_sizes_desc, PromptArtifacts};

/// Approximates token count from character count.
pub fn estimate_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

/// Prints the total static request footprint for one example case.
pub fn print_footprint(label: &str, artifacts: &PromptArtifacts) {
    println!("{label}:");
    println!(
        "  System prompt: {} chars, {} lines, ~{} tokens",
        artifacts.system_prompt.len(),
        artifacts.system_prompt.lines().count(),
        estimate_tokens(artifacts.system_prompt.len())
    );
    println!(
        "  Tool definitions: {} chars, {} tools, ~{} tokens",
        artifacts.tool_definition_payload.len(),
        artifacts.tool_definitions.len(),
        estimate_tokens(artifacts.tool_definition_payload.len())
    );
    println!(
        "  Combined static input: {} chars, ~{} tokens",
        artifacts.total_chars(),
        estimate_tokens(artifacts.total_chars())
    );
}

/// Prints a sorted size breakdown.
pub fn print_ranked_sizes(title: &str, sizes: &[(String, usize)]) {
    println!("\n{title}");
    for (name, chars) in sizes {
        println!(
            "  {name}: {chars} chars (~{} tokens)",
            estimate_tokens(*chars)
        );
    }
}

/// Returns rendered tool-guideline section sizes sorted from largest to smallest.
pub fn section_sizes(artifacts: &PromptArtifacts) -> Vec<(String, usize)> {
    let mut sections = artifacts.guideline_sections.clone();
    sort_sizes_desc(&mut sections);
    sections
}

pub fn print_tool_definitions(artifacts: &super::PromptArtifacts) {
    println!("\n{}", "=".repeat(60));
    println!("Tool Definitions:");
    for tool in &artifacts.tool_definitions {
        let name = tool["name"].as_str().unwrap_or("unknown");
        println!("\n--- {name} ---");
        println!("{}", serde_json::to_string_pretty(tool).unwrap());
    }
}

pub(super) fn collect_guideline_sections(prompt: &str) -> Vec<(String, usize)> {
    let mut in_guidelines = false;
    let mut current_name: Option<String> = None;
    let mut current_len = 0;
    let mut sections = Vec::new();

    for line in prompt.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        if trimmed == "# Tool Usage Guidelines" {
            in_guidelines = true;
            continue;
        }
        if in_guidelines && trimmed.starts_with("# ") && !trimmed.starts_with("## ") {
            break;
        }
        if !in_guidelines {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("## ") {
            if let Some(previous) = current_name.take() {
                sections.push((previous, current_len));
            }
            let name = name.strip_suffix(" Tool").unwrap_or(name).trim_matches('`');
            current_name = Some(name.to_string());
            current_len = line.len();
            continue;
        }
        if current_name.is_some() {
            current_len += line.len();
        }
    }

    if let Some(name) = current_name {
        sections.push((name, current_len));
    }

    sections
}
