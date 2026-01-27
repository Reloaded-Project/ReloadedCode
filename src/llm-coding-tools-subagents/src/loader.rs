//! Agent configuration loader with directory scanning.

use crate::config::{AgentConfig, RawFrontmatter};
use crate::error::{AgentConfigError, AgentConfigResult};
use crate::frontmatter::parse_frontmatter;
use crate::registry::SubagentRegistry;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Builder for loading agent configs from files, directories, and in-memory configs into a [`SubagentRegistry`].
///
/// [`AgentLoader`] provides a flexible way to assemble a [`SubagentRegistry`] from multiple sources:
/// - Directories (scanned for `agent/**/*.md` and `agents/**/*.md`)
/// - Individual files (names derived from file names, with optional override)
/// - In-memory [`AgentConfig`] entries
///
/// Later sources override earlier entries with the same name.
///
/// # Example
///
/// ```no_run
/// use llm_coding_tools_subagents::AgentLoader;
/// use std::path::Path;
///
/// let mut loader = AgentLoader::new();
/// loader.add_directory(Path::new("~/.opencode"));
/// loader.add_file(Path::new("/path/to/custom_agent.md"));
///
/// let registry = loader.load().unwrap();
/// ```
#[derive(Debug, Clone, Default)]
pub struct AgentLoader {
    sources: Vec<AgentSource>,
}

/// Internal source enum to preserve insertion order and override semantics.
#[derive(Debug, Clone)]
enum AgentSource {
    Directory(PathBuf),
    File { path: PathBuf, name: Option<String> },
    Config(Box<AgentConfig>),
}

impl AgentLoader {
    /// Creates an empty loader.
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// Creates a loader with preallocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sources: Vec::with_capacity(capacity),
        }
    }

    /// Adds a directory to scan for `agent/**/*.md` or `agents/**/*.md`.
    pub fn add_directory(&mut self, directory: impl Into<PathBuf>) -> &mut Self {
        self.sources.push(AgentSource::Directory(directory.into()));
        self
    }

    /// Adds a single agent file (name derived from file name).
    pub fn add_file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.sources.push(AgentSource::File {
            path: path.into(),
            name: None,
        });
        self
    }

    /// Adds a single agent file with an explicit name override.
    pub fn add_file_named(
        &mut self,
        path: impl Into<PathBuf>,
        name: impl Into<String>,
    ) -> &mut Self {
        self.sources.push(AgentSource::File {
            path: path.into(),
            name: Some(name.into()),
        });
        self
    }

    /// Adds an in-memory [`AgentConfig`].
    pub fn add_config(&mut self, config: AgentConfig) -> &mut Self {
        self.sources.push(AgentSource::Config(Box::new(config)));
        self
    }

    /// Loads all configured sources into a new [`SubagentRegistry`].
    pub fn load(self) -> AgentConfigResult<SubagentRegistry> {
        let mut registry = SubagentRegistry::new();
        self.load_into_registry(&mut registry)?;
        Ok(registry)
    }

    /// Loads all configured sources into an existing [`SubagentRegistry`].
    /// Later sources override earlier entries.
    pub fn load_into_registry(self, registry: &mut SubagentRegistry) -> AgentConfigResult<()> {
        let additional = self
            .sources
            .iter()
            .filter(|source| !matches!(source, AgentSource::Directory(_)))
            .count();
        registry.reserve(additional);

        for source in self.sources {
            match source {
                AgentSource::Directory(dir) => {
                    load_directory_into_registry(registry, &dir)?;
                }
                AgentSource::File { path, name } => {
                    let override_name = name;
                    let derived_name = path
                        .file_stem()
                        .map(|stem: &std::ffi::OsStr| stem.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if derived_name.is_empty() {
                        return Err(AgentConfigError::SchemaValidation {
                            path: path.to_path_buf(),
                            message: "agent file name is empty".to_string(),
                        });
                    }

                    let mut config = load_agent_file(&path, derived_name)?;
                    if let Some(name) = override_name {
                        config.name = name;
                    }
                    registry.insert(config);
                }
                AgentSource::Config(config) => {
                    registry.insert(*config);
                }
            }
        }
        Ok(())
    }
}

fn load_directory_into_registry(
    registry: &mut SubagentRegistry,
    dir: &Path,
) -> AgentConfigResult<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    // NOTE: keep this walker configuration identical to the existing load_agents.
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

        let Some(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            continue;
        }

        let path = entry.path();
        let rel_path = match path.strip_prefix(dir) {
            Ok(p) => p.to_string_lossy(),
            Err(_) => continue,
        };

        #[cfg(windows)]
        let rel_path = rel_path.replace('\\', "/");
        #[cfg(not(windows))]
        let rel_path = rel_path.into_owned();

        if !matches_agent_pattern(&rel_path) {
            continue;
        }

        let name = derive_agent_name_from_rel(&rel_path);
        let config = load_agent_file(path, name)?;
        registry.insert(config);
    }

    Ok(())
}

/// Loads agent configs from directories into a [`SubagentRegistry`].
///
/// Scans for `agent/**/*.md` and `agents/**/*.md` under each directory.
/// Later directories override earlier entries with the same name.
pub fn load_agents_registry(directories: &[&Path]) -> AgentConfigResult<SubagentRegistry> {
    let mut loader = AgentLoader::with_capacity(directories.len());
    for dir in directories {
        loader.add_directory(*dir);
    }
    loader.load()
}

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
pub fn load_agents(directories: &[&Path]) -> AgentConfigResult<HashMap<String, AgentConfig>> {
    let registry = load_agents_registry(directories)?;
    Ok(registry.into_map())
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
    use crate::config::AgentMode;
    use indexmap::IndexMap;
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

    fn make_agent(name: &str, description: &str) -> AgentConfig {
        AgentConfig {
            name: name.to_string(),
            mode: AgentMode::Subagent,
            description: description.to_string(),
            model: None,
            hidden: false,
            temperature: None,
            top_p: None,
            permission: IndexMap::new(),
            options: HashMap::new(),
            prompt: String::new(),
        }
    }

    #[test]
    fn agent_loader_file_name_cases() {
        let cases = [
            (
                "custom/example.md",
                "---\nmode: subagent\n---\nBody",
                None,
                "example",
            ),
            (
                "custom/agent.md",
                "---\nmode: subagent\n---\nBody",
                Some("override/name"),
                "override/name",
            ),
            (
                "custom/agent.md",
                "---\nname: frontmatter-name\nmode: subagent\n---\nBody",
                Some("override/name"),
                "override/name",
            ),
        ];

        for (rel_path, content, override_name, expected) in cases {
            let dir = TempDir::new().unwrap();
            create_agent_file(dir.path(), rel_path, content);

            let mut loader = AgentLoader::new();
            let full_path = dir.path().join(rel_path);
            match override_name {
                Some(name) => {
                    loader.add_file_named(full_path, name);
                }
                None => {
                    loader.add_file(full_path);
                }
            }

            let registry = loader.load().unwrap();
            assert!(registry.get(expected).is_some());
        }
    }

    #[test]
    fn agent_loader_allows_in_memory_config_and_overrides() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "custom/agent.md",
            "---\nmode: subagent\ndescription: First\n---\nBody",
        );

        let mut loader = AgentLoader::new();
        loader.add_file(dir.path().join("custom/agent.md"));
        loader.add_config(make_agent("agent", "Second"));

        let registry = loader.load().unwrap();
        assert_eq!(registry.get("agent").unwrap().description, "Second");
    }

    #[test]
    fn agent_loader_loads_into_existing_registry() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("existing", "keep"));

        let mut loader = AgentLoader::new();
        loader.add_config(make_agent("new", "added"));
        loader.load_into_registry(&mut registry).unwrap();

        assert!(registry.get("existing").is_some());
        assert!(registry.get("new").is_some());
    }

    #[test]
    fn agent_loader_loads_explicit_file_without_agent_prefix() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "custom/explicit.md",
            "---\nmode: subagent\ndescription: Explicit\n---\nBody",
        );

        let mut loader = AgentLoader::new();
        loader.add_file(dir.path().join("custom/explicit.md"));

        let registry = loader.load().unwrap();
        let agent = registry.get("explicit").unwrap();
        assert_eq!(agent.description, "Explicit");
    }

    #[test]
    fn agent_loader_scans_directories_with_agent_patterns() {
        let dir = TempDir::new().unwrap();
        create_agent_file(dir.path(), "agent/one.md", "---\nmode: subagent\n---\nOne");
        create_agent_file(
            dir.path(),
            "agents/nested/two.md",
            "---\nmode: primary\n---\nTwo",
        );

        let mut loader = AgentLoader::new();
        loader.add_directory(dir.path());

        let registry = loader.load().unwrap();
        assert!(registry.get("one").is_some());
        assert!(registry.get("nested/two").is_some());
    }

    #[test]
    fn agent_loader_overrides_existing_registry_entries() {
        // Later insertions (from the loader) override earlier registry entries with the same name.
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("override", "old"));

        let mut loader = AgentLoader::new();
        loader.add_config(make_agent("override", "new"));
        loader.load_into_registry(&mut registry).unwrap();

        assert_eq!(registry.get("override").unwrap().description, "new");
    }
}
