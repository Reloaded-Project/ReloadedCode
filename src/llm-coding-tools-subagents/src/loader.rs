//! Agent configuration loader with directory scanning.

use crate::config::{AgentConfig, RawFrontmatter};
use crate::error::{AgentConfigError, AgentConfigResult};
use crate::frontmatter::parse_frontmatter;
use ignore::WalkBuilder;
use indexmap::IndexMap;
use std::fs;
use std::path::Path;

/// Loads all agent configurations from the given directories.
///
/// Scans each directory for files matching `agent/**/*.md` or `agents/**/*.md`,
/// parses frontmatter, and returns a map keyed by agent name.
///
/// Agent names are derived from file paths relative to the scan directory by
/// stripping the `agent/` or `agents/` prefix and `.md` extension. For example:
/// - `<dir>/agent/mcp-search.md` -> `"mcp-search"`
/// - `<dir>/agents/orchestrator/builder.md` -> `"orchestrator/builder"`
///
/// # Errors
///
/// Returns the first error encountered when parsing agent files.
/// Files that fail to parse will stop the loading process.
pub fn load_agents(directories: &[&Path]) -> AgentConfigResult<IndexMap<String, AgentConfig>> {
    let mut agents = IndexMap::new();

    for dir in directories {
        if !dir.is_dir() {
            continue;
        }

        let walker = WalkBuilder::new(dir)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(true)
            .build();

        for entry_result in walker {
            let entry = match entry_result {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Skip directories
            let Some(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                continue;
            }

            let path = entry.path();

            // Get path relative to search dir for pattern matching
            let rel_path = match path.strip_prefix(dir) {
                Ok(p) => p.to_string_lossy(),
                Err(_) => continue,
            };

            // Normalize to forward slashes for cross-platform matching
            #[cfg(windows)]
            let rel_path = rel_path.replace('\\', "/");
            #[cfg(not(windows))]
            let rel_path = rel_path.into_owned();

            // Check if this is an agent file
            if !matches_agent_pattern(&rel_path) {
                continue;
            }

            // FIX #4: Derive name from rel_path, not absolute path
            let name = derive_agent_name_from_rel(&rel_path);
            let config = load_agent_file(path, name)?;
            agents.insert(config.name.clone(), config);
        }
    }

    Ok(agents)
}

/// Loads a single agent configuration from a file.
fn load_agent_file(path: &Path, name: String) -> AgentConfigResult<AgentConfig> {
    let content = fs::read_to_string(path).map_err(|e| AgentConfigError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let result = parse_frontmatter::<RawFrontmatter>(content, path)?;

    Ok(AgentConfig::from_raw(name, result.data, result.content))
}

/// Checks if a relative path matches `agent/**/*.md` or `agents/**/*.md`.
fn matches_agent_pattern(rel_path: &str) -> bool {
    let is_agent_dir = rel_path.starts_with("agent/") || rel_path.starts_with("agents/");
    let is_md_file = rel_path.ends_with(".md");
    is_agent_dir && is_md_file
}

/// Derives agent name from relative path.
///
/// FIX #4: Use rel_path (relative to scan root) instead of absolute path.
/// Strips leading `agent/` or `agents/` segment and `.md` extension.
///
/// Examples:
/// - `agent/test.md` -> `"test"`
/// - `agents/nested/deep.md` -> `"nested/deep"`
fn derive_agent_name_from_rel(rel_path: &str) -> String {
    let without_prefix = rel_path
        .strip_prefix("agent/")
        .or_else(|| rel_path.strip_prefix("agents/"))
        .unwrap_or(rel_path);

    without_prefix
        .strip_suffix(".md")
        .unwrap_or(without_prefix)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn create_agent_file(dir: &Path, rel_path: &str, content: &str) {
        let full_path = dir.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(full_path).unwrap();
        write!(file, "{}", content).unwrap();
    }

    #[test]
    fn matches_agent_pattern_works() {
        assert!(matches_agent_pattern("agent/test.md"));
        assert!(matches_agent_pattern("agents/test.md"));
        assert!(matches_agent_pattern("agent/nested/deep.md"));
        assert!(matches_agent_pattern("agents/nested/deep.md"));
        assert!(!matches_agent_pattern("other/test.md"));
        assert!(!matches_agent_pattern("agent/test.txt"));
        assert!(!matches_agent_pattern("notagen/test.md"));
    }

    #[test]
    fn derive_agent_name_from_rel_works() {
        assert_eq!(derive_agent_name_from_rel("agent/test.md"), "test");
        assert_eq!(derive_agent_name_from_rel("agents/test.md"), "test");
        assert_eq!(
            derive_agent_name_from_rel("agent/nested/deep.md"),
            "nested/deep"
        );
        assert_eq!(
            derive_agent_name_from_rel("agents/foo/bar/baz.md"),
            "foo/bar/baz"
        );
    }

    #[test]
    fn load_agents_derives_name_from_rel_path_not_absolute() {
        // FIX #4: Even if base path contains /agent/, name is derived from rel_path
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test-agent.md",
            "---\nmode: subagent\ndescription: Test\n---\nPrompt",
        );

        let agents = load_agents(&[dir.path()]).unwrap();

        assert_eq!(agents.len(), 1);
        // Name should be "test-agent", not something derived from absolute path
        assert!(agents.contains_key("test-agent"));
    }

    #[test]
    fn load_agents_finds_files_in_agent_dir() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test-agent.md",
            "---\nmode: subagent\ndescription: Test\n---\nPrompt",
        );

        let agents = load_agents(&[dir.path()]).unwrap();

        assert_eq!(agents.len(), 1);
        assert!(agents.contains_key("test-agent"));
        assert_eq!(agents["test-agent"].description, "Test");
        assert_eq!(agents["test-agent"].prompt, "Prompt");
    }

    #[test]
    fn load_agents_finds_files_in_agents_dir() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agents/nested/deep.md",
            "---\nmode: primary\n---\nBody",
        );

        let agents = load_agents(&[dir.path()]).unwrap();

        assert_eq!(agents.len(), 1);
        assert!(agents.contains_key("nested/deep"));
    }

    #[test]
    fn load_agents_ignores_non_md_files() {
        let dir = TempDir::new().unwrap();
        create_agent_file(dir.path(), "agent/readme.txt", "not an agent");
        create_agent_file(
            dir.path(),
            "agent/real.md",
            "---\nmode: subagent\n---\nReal",
        );

        let agents = load_agents(&[dir.path()]).unwrap();

        assert_eq!(agents.len(), 1);
        assert!(agents.contains_key("real"));
    }

    #[test]
    fn load_agents_ignores_files_outside_agent_dirs() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "other/file.md",
            "---\nmode: subagent\n---\nBody",
        );

        let agents = load_agents(&[dir.path()]).unwrap();

        assert!(agents.is_empty());
    }

    #[test]
    fn load_agents_scans_multiple_directories() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        create_agent_file(dir1.path(), "agent/first.md", "---\nmode: subagent\n---\n");
        create_agent_file(dir2.path(), "agent/second.md", "---\nmode: primary\n---\n");

        let agents = load_agents(&[dir1.path(), dir2.path()]).unwrap();

        assert_eq!(agents.len(), 2);
        assert!(agents.contains_key("first"));
        assert!(agents.contains_key("second"));
    }

    #[test]
    fn load_agents_handles_model_with_colons() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test.md",
            "---\nmodel: provider/model:tag\nmode: subagent\n---\nBody",
        );

        let agents = load_agents(&[dir.path()]).unwrap();

        assert_eq!(agents["test"].model, Some("provider/model:tag".to_string()));
    }

    #[test]
    fn load_agents_parses_permissions() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/perms.md",
            "---\nmode: subagent\npermission:\n  bash: allow\n  task: deny\n---\n",
        );

        let agents = load_agents(&[dir.path()]).unwrap();
        let perms = &agents["perms"].permission;

        assert_eq!(perms.len(), 2);
    }

    #[test]
    fn load_agents_handles_flow_permission_syntax() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/flow.md",
            "---\nmode: subagent\npermission:\n  task: { \"*\": \"deny\" }\n---\n",
        );

        let agents = load_agents(&[dir.path()]).unwrap();
        // Should parse without error (flow syntax preserved)
        assert!(agents.contains_key("flow"));
    }
}
