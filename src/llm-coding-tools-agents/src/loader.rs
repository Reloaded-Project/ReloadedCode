//! Agent configuration loader with directory scanning.

use crate::catalog::AgentCatalog;
use crate::config::{AgentConfig, RawFrontmatter};
use crate::error::{AgentLoadError, AgentLoadResult};
use crate::parser::{parse_agent, AgentParseError};
use crate::registry::SubagentRegistry;
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};

/// Stateless loader for parsing and inserting agent configs into a [`SubagentRegistry`] or [`AgentCatalog`].
///
/// [`AgentLoader`] provides a flexible way to assemble a [`SubagentRegistry`] or [`AgentCatalog`] from multiple sources:
/// - Directories (scanned for `agent/**/*.md` and `agents/**/*.md`)
/// - Individual files (names derived from file names, with optional override)
/// - In-memory [`AgentConfig`] entries
///
/// Later insertions override earlier entries with the same name.
///
/// # Example
///
/// ```no_run
/// use llm_coding_tools_agents::{AgentLoader, SubagentRegistry};
/// use std::path::Path;
///
/// let mut loader = AgentLoader::new();
/// let mut registry = SubagentRegistry::new();
/// loader.add_directory(&mut registry, Path::new("~/.opencode"))?;
/// loader.add_file(&mut registry, Path::new("/path/to/custom_agent.md"))?;
/// # Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentLoader;

impl AgentLoader {
    /// Creates a new stateless loader.
    pub fn new() -> Self {
        Self
    }

    /// Adds all agents from a directory to the registry.
    ///
    /// # Arguments
    ///
    /// * `registry` - The registry to insert agents into
    /// * `directory` - Root directory to scan for `agent/**/*.md` and `agents/**/*.md`
    pub fn add_directory(
        &self,
        registry: &mut SubagentRegistry,
        directory: impl Into<PathBuf>,
    ) -> AgentLoadResult<()> {
        let dir = directory.into();
        load_directory_with(&dir, |path, name| {
            let config = load_agent_file(path, name.to_string())?;
            registry.insert(config);
            Ok(())
        })
    }

    /// Adds a single agent file (name derived from file name) to the registry.
    ///
    /// # Arguments
    ///
    /// * `registry` - The registry to insert the agent into
    /// * `path` - Path to a markdown file with YAML frontmatter
    pub fn add_file(
        &self,
        registry: &mut SubagentRegistry,
        path: impl Into<PathBuf>,
    ) -> AgentLoadResult<()> {
        let path = path.into();
        let derived_name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if derived_name.is_empty() {
            return Err(AgentLoadError::SchemaValidation {
                path: path.to_path_buf(),
                message: "agent file name is empty".to_string(),
            });
        }
        let config = load_agent_file(&path, derived_name)?;
        registry.insert(config);
        Ok(())
    }

    /// Adds a single agent file with an explicit name override to the registry.
    ///
    /// The explicit name always overrides any frontmatter `name` field.
    ///
    /// # Arguments
    ///
    /// * `registry` - The registry to insert the agent into
    /// * `path` - Path to a markdown file with YAML frontmatter
    /// * `name` - Explicit agent name to use
    pub fn add_file_named(
        &self,
        registry: &mut SubagentRegistry,
        path: impl Into<PathBuf>,
        name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let path = path.into();
        let override_name = name.into();
        if override_name.is_empty() {
            return Err(AgentLoadError::SchemaValidation {
                path: path.to_path_buf(),
                message: "agent name is empty".to_string(),
            });
        }
        let mut config = load_agent_file(&path, String::new())?;
        config.name = override_name;
        registry.insert(config);
        Ok(())
    }

    /// Adds an in-memory [`AgentConfig`] to the registry.
    ///
    /// # Arguments
    ///
    /// * `registry` - The registry to insert the agent into
    /// * `config` - Fully constructed agent configuration
    pub fn add_config(
        &self,
        registry: &mut SubagentRegistry,
        config: AgentConfig,
    ) -> AgentLoadResult<()> {
        registry.insert(config);
        Ok(())
    }

    /// Adds an agent configuration from a raw markdown string to the registry.
    ///
    /// The string should contain YAML frontmatter delimited by `---` followed
    /// by the prompt body. The agent name is derived from the `name` field
    /// in the frontmatter if present; otherwise, `default_name` is used.
    ///
    /// # Arguments
    ///
    /// * `registry` - The registry to insert the agent into
    /// * `markdown` - Raw markdown string with YAML frontmatter
    /// * `default_name` - Agent name to use if not specified in frontmatter
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Parsing fails (propagates the underlying parse error)
    /// - The resulting agent name is empty
    pub fn add_from_str(
        &self,
        registry: &mut SubagentRegistry,
        markdown: impl Into<String>,
        default_name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let content = markdown.into();
        let name = default_name.into();

        let config = parse_agent_config(content, name).map_err(|err| AgentLoadError::Parse {
            path: PathBuf::from("<memory>"),
            source: err,
        })?;

        if config.name.is_empty() {
            return Err(AgentLoadError::SchemaValidation {
                path: PathBuf::from("<memory>"),
                message: "agent name is empty".to_string(),
            });
        }

        registry.insert(config);
        Ok(())
    }

    /// Adds an agent configuration from raw markdown bytes to the registry.
    ///
    /// A convenience wrapper around [`Self::add_from_str`] that converts bytes to UTF-8 string.
    /// Invalid UTF-8 bytes will result in a schema validation error.
    ///
    /// # Arguments
    ///
    /// * `registry` - The registry to insert the agent into
    /// * `bytes` - Raw markdown bytes with YAML frontmatter
    /// * `default_name` - Agent name to use if not specified in frontmatter
    pub fn add_from_bytes(
        &self,
        registry: &mut SubagentRegistry,
        bytes: impl AsRef<[u8]>,
        default_name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        match std::str::from_utf8(bytes.as_ref()) {
            Ok(content) => self.add_from_str(registry, content, default_name),
            Err(err) => Err(AgentLoadError::SchemaValidation {
                path: PathBuf::from("<memory>"),
                message: format!("invalid UTF-8: {err}"),
            }),
        }
    }

    // ========== Catalog Methods ==========

    /// Adds all agents from a directory to the catalog.
    ///
    /// Parameters:
    /// - `catalog`: the catalog to insert agents into
    /// - `directory`: root directory to scan for `agent/**/*.md` and `agents/**/*.md`
    ///
    /// Returns: `Ok(())` on success or [`AgentLoadError`] on failure.
    ///
    /// Unreadable directory entries are skipped to preserve loader parity.
    pub fn add_directory_to_catalog(
        &self,
        catalog: &mut AgentCatalog,
        directory: impl Into<PathBuf>,
    ) -> AgentLoadResult<()> {
        let dir = directory.into();
        load_directory_with(&dir, |path, name| {
            let config = load_agent_file(path, name.to_string())?;
            catalog.insert(config);
            Ok(())
        })
    }

    /// Adds a single agent file (name derived from file name) to the catalog.
    ///
    /// Parameters:
    /// - `catalog`: the catalog to insert the agent into
    /// - `path`: path to a markdown file with YAML frontmatter
    ///
    /// Returns: `Ok(())` on success or [`AgentLoadError`] on failure.
    pub fn add_file_to_catalog(
        &self,
        catalog: &mut AgentCatalog,
        path: impl Into<PathBuf>,
    ) -> AgentLoadResult<()> {
        let path = path.into();
        let derived_name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        if derived_name.is_empty() {
            return Err(AgentLoadError::SchemaValidation {
                path: path.to_path_buf(),
                message: "agent file name is empty".to_string(),
            });
        }
        let config = load_agent_file(&path, derived_name)?;
        catalog.insert(config);
        Ok(())
    }

    /// Adds a single agent file with an explicit name override to the catalog.
    ///
    /// Parameters:
    /// - `catalog`: the catalog to insert the agent into
    /// - `path`: path to a markdown file with YAML frontmatter
    /// - `name`: explicit agent name to use
    ///
    /// Returns: `Ok(())` on success or [`AgentLoadError`] on failure.
    pub fn add_file_named_to_catalog(
        &self,
        catalog: &mut AgentCatalog,
        path: impl Into<PathBuf>,
        name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let path = path.into();
        let override_name = name.into();
        if override_name.is_empty() {
            return Err(AgentLoadError::SchemaValidation {
                path: path.to_path_buf(),
                message: "agent name is empty".to_string(),
            });
        }
        let mut config = load_agent_file(&path, String::new())?;
        config.name = override_name;
        catalog.insert(config);
        Ok(())
    }

    /// Adds an in-memory [`AgentConfig`] to the catalog.
    ///
    /// Parameters:
    /// - `catalog`: the catalog to insert the agent into
    /// - `config`: fully constructed agent configuration
    ///
    /// Returns: Ok(()) on success.
    pub fn add_config_to_catalog(
        &self,
        catalog: &mut AgentCatalog,
        config: AgentConfig,
    ) -> AgentLoadResult<()> {
        catalog.insert(config);
        Ok(())
    }

    /// Adds an agent configuration from a raw markdown string to the catalog.
    ///
    /// Parameters:
    /// - `catalog`: the catalog to insert the agent into
    /// - `markdown`: raw markdown string with YAML frontmatter
    /// - `default_name`: agent name to use if not specified in frontmatter
    ///
    /// Returns: `Ok(())` on success or [`AgentLoadError`] on failure.
    ///
    /// Catalogs are config-only and must not synthesize hidden error agents.
    /// This stricter failure behavior is intentional to satisfy the
    /// "no placeholder types/errors" constraint while producing a clean
    /// config collection for framework registries. The registry loader
    /// keeps its existing hidden-error-agent behavior unchanged.
    pub fn add_from_str_to_catalog(
        &self,
        catalog: &mut AgentCatalog,
        markdown: impl Into<String>,
        default_name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let config = config_from_str_strict(markdown, default_name)?;
        catalog.insert(config);
        Ok(())
    }

    /// Adds an agent configuration from raw markdown bytes to the catalog.
    ///
    /// Parameters:
    /// - `catalog`: the catalog to insert the agent into
    /// - `bytes`: raw markdown bytes with YAML frontmatter
    /// - `default_name`: agent name to use if not specified in frontmatter
    ///
    /// Returns: `Ok(())` on success or [`AgentLoadError`] on failure.
    ///
    /// Catalogs are config-only and must not synthesize hidden error agents.
    /// Invalid UTF-8 is surfaced as a validation error to keep the catalog
    /// free of placeholder configs. The registry loader remains unchanged.
    pub fn add_from_bytes_to_catalog(
        &self,
        catalog: &mut AgentCatalog,
        bytes: impl AsRef<[u8]>,
        default_name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let content = std::str::from_utf8(bytes.as_ref()).map_err(|err| {
            AgentLoadError::SchemaValidation {
                path: PathBuf::from("<memory>"),
                message: format!("invalid UTF-8: {err}"),
            }
        })?;
        let config = config_from_str_strict(content, default_name)?;
        catalog.insert(config);
        Ok(())
    }
}

/// Shared directory scan helper used by both registry and catalog loading.
fn load_directory_with(
    dir: &Path,
    mut on_match: impl FnMut(&Path, &str) -> AgentLoadResult<()>,
) -> AgentLoadResult<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    // Keep walker config identical to existing registry behavior.
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
            Err(_) => continue, // preserve existing behavior: skip unreadable entries
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

        // Skip entries that would produce empty agent names (e.g., agent/.md)
        let name = match derive_agent_name_from_rel(&rel_path) {
            Some(n) => n,
            None => continue,
        };

        on_match(path, name.as_str())?;
    }

    Ok(())
}

/// Shared parse helper that reuses existing loader parsing in both registry + catalog.
fn parse_agent_config(
    content: String,
    default_name: String,
) -> Result<AgentConfig, AgentParseError> {
    let result = parse_agent::<RawFrontmatter>(content)?;
    Ok(AgentConfig::from_raw(
        default_name,
        result.data,
        result.content,
    ))
}

/// Loads a single agent configuration from a file.
fn load_agent_file(path: &Path, name: String) -> AgentLoadResult<AgentConfig> {
    let content = fs::read_to_string(path).map_err(|e| AgentLoadError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    parse_agent_config(content, name).map_err(|err| AgentLoadError::Parse {
        path: path.to_path_buf(),
        source: err,
    })
}

/// Strict parser for catalog-only string loading (validates non-empty name).
fn config_from_str_strict(
    markdown: impl Into<String>,
    default_name: impl Into<String>,
) -> AgentLoadResult<AgentConfig> {
    let name = default_name.into();
    let config =
        parse_agent_config(markdown.into(), name.clone()).map_err(|err| AgentLoadError::Parse {
            path: PathBuf::from("<memory>"),
            source: err,
        })?;

    if config.name.is_empty() {
        return Err(AgentLoadError::SchemaValidation {
            path: PathBuf::from("<memory>"),
            message: "agent name is empty".to_string(),
        });
    }

    Ok(config)
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
/// - `agent/.md` -> `None` (empty name)
fn derive_agent_name_from_rel(rel_path: &str) -> Option<String> {
    let without_prefix = rel_path
        .strip_prefix("agent/")
        .or_else(|| rel_path.strip_prefix("agents/"))
        .unwrap_or(rel_path);

    let name = without_prefix
        .strip_suffix(".md")
        .unwrap_or(without_prefix)
        .to_string();

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::AgentCatalog;
    use crate::config::AgentMode;
    use indexmap::IndexMap;
    use std::collections::HashMap;
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
        assert_eq!(
            derive_agent_name_from_rel("agent/test.md"),
            Some("test".to_string())
        );
        assert_eq!(
            derive_agent_name_from_rel("agents/test.md"),
            Some("test".to_string())
        );
        assert_eq!(
            derive_agent_name_from_rel("agent/nested/deep.md"),
            Some("nested/deep".to_string())
        );
        assert_eq!(
            derive_agent_name_from_rel("agents/foo/bar/baz.md"),
            Some("foo/bar/baz".to_string())
        );
        // Empty name edge case
        assert_eq!(derive_agent_name_from_rel("agent/.md"), None);
        assert_eq!(derive_agent_name_from_rel("agents/.md"), None);
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

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert_eq!(registry.len(), 1);
        // Name should be "test-agent", not something derived from absolute path
        assert!(registry.get("test-agent").is_some());
    }

    #[test]
    fn load_agents_finds_files_in_agent_dir() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test-agent.md",
            "---\nmode: subagent\ndescription: Test\n---\nPrompt",
        );

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.get("test-agent").is_some());
        assert_eq!(registry.get("test-agent").unwrap().description, "Test");
        assert_eq!(registry.get("test-agent").unwrap().prompt, "Prompt");
    }

    #[test]
    fn load_agents_finds_files_in_agents_dir() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agents/nested/deep.md",
            "---\nmode: primary\n---\nBody",
        );

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.get("nested/deep").is_some());
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

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.get("real").is_some());
    }

    #[test]
    fn load_agents_ignores_files_outside_agent_dirs() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "other/file.md",
            "---\nmode: subagent\n---\nBody",
        );

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert!(registry.is_empty());
    }

    #[test]
    fn load_agents_scans_multiple_directories() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        create_agent_file(dir1.path(), "agent/first.md", "---\nmode: subagent\n---\n");
        create_agent_file(dir2.path(), "agent/second.md", "---\nmode: primary\n---\n");

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir1.path()).unwrap();
        loader.add_directory(&mut registry, dir2.path()).unwrap();

        assert_eq!(registry.len(), 2);
        assert!(registry.get("first").is_some());
        assert!(registry.get("second").is_some());
    }

    #[test]
    fn load_agents_handles_model_with_colons() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test.md",
            "---\nmodel: provider/model:tag\nmode: subagent\n---\nBody",
        );

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert_eq!(
            registry.get("test").unwrap().model,
            Some("provider/model:tag".to_string())
        );
    }

    #[test]
    fn load_agents_parses_permissions() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/perms.md",
            "---\nmode: subagent\npermission:\n  bash: allow\n  task: deny\n---\n",
        );

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();
        let perms = &registry.get("perms").unwrap().permission;

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

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();
        // Should parse without error (flow syntax preserved)
        assert!(registry.get("flow").is_some());
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

            let loader = AgentLoader::new();
            let mut registry = SubagentRegistry::new();
            let full_path = dir.path().join(rel_path);
            match override_name {
                Some(name) => {
                    loader
                        .add_file_named(&mut registry, full_path, name)
                        .unwrap();
                }
                None => {
                    loader.add_file(&mut registry, full_path).unwrap();
                }
            }

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

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader
            .add_file(&mut registry, dir.path().join("custom/agent.md"))
            .unwrap();
        loader
            .add_config(&mut registry, make_agent("agent", "Second"))
            .unwrap();

        assert_eq!(registry.get("agent").unwrap().description, "Second");
    }

    #[test]
    fn agent_loader_loads_into_existing_registry() {
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("existing", "keep"));

        let loader = AgentLoader::new();
        loader
            .add_config(&mut registry, make_agent("new", "added"))
            .unwrap();

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

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader
            .add_file(&mut registry, dir.path().join("custom/explicit.md"))
            .unwrap();

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

        let loader = AgentLoader::new();
        let mut registry = SubagentRegistry::new();
        loader.add_directory(&mut registry, dir.path()).unwrap();

        assert!(registry.get("one").is_some());
        assert!(registry.get("nested/two").is_some());
    }

    #[test]
    fn agent_loader_overrides_existing_registry_entries() {
        // Later insertions (from the loader) override earlier registry entries with the same name.
        let mut registry = SubagentRegistry::new();
        registry.insert(make_agent("override", "old"));

        let loader = AgentLoader::new();
        loader
            .add_config(&mut registry, make_agent("override", "new"))
            .unwrap();

        assert_eq!(registry.get("override").unwrap().description, "new");
    }

    // ========== Catalog Tests ==========

    #[test]
    fn catalog_loads_agent_dir_pattern() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/single.md",
            "---\nmode: subagent\n---\nBody",
        );
        create_agent_file(
            dir.path(),
            "agents/nested/deep.md",
            "---\nmode: primary\n---\nBody",
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader
            .add_directory_to_catalog(&mut catalog, dir.path())
            .unwrap();

        assert!(catalog.by_name("single").is_some());
        assert!(catalog.by_name("nested/deep").is_some());
    }

    #[test]
    fn catalog_add_file_uses_file_stem() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "custom/explicit.md",
            "---\nmode: subagent\n---\nBody",
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader
            .add_file_to_catalog(&mut catalog, dir.path().join("custom/explicit.md"))
            .unwrap();

        assert!(catalog.by_name("explicit").is_some());
    }

    #[test]
    fn catalog_overwrites_existing_entries_last_wins() {
        // Reuse the existing make_agent(name, description) helper in this test module.
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "custom/agent.md",
            "---\nmode: subagent\ndescription: First\n---\nBody",
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader
            .add_file_to_catalog(&mut catalog, dir.path().join("custom/agent.md"))
            .unwrap();
        loader
            .add_config_to_catalog(&mut catalog, make_agent("agent", "Second"))
            .unwrap();

        assert_eq!(catalog.by_name("agent").unwrap().description, "Second");
    }
}
