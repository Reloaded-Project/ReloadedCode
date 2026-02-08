//! Agent configuration loader with directory scanning.
//!
//! Provides [`AgentLoader`] for populating [`AgentCatalog`] from multiple sources:
//! directories (`agent/**/*.md`, `agents/**/*.md`), individual files, strings, or
//! in-memory configs. Later insertions override earlier entries with the same name.
//!
//! The loader is stateless and reusable—create once with [`AgentLoader::new()`]
//! and populate multiple catalogs.
//!
//! # Example
//!
//! ```no_run
//! use llm_coding_tools_agents::{AgentLoader, AgentCatalog};
//! use std::path::Path;
//!
//! let loader = AgentLoader::new();
//! let mut catalog = AgentCatalog::new();
//!
//! // Scan directories for agent definitions
//! loader.add_directory(&mut catalog, Path::new("~/.opencode"))?;
//!
//! // Load specific files
//! loader.add_file(&mut catalog, Path::new("custom.md"))?;
//!
//! // Parse from string (useful for embedded configs)
//! loader.add_from_str(&mut catalog, "---\nmode: subagent\n---\nprompt", "agent-name")?;
//! # Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
//! ```

use crate::catalog::AgentCatalog;
use crate::config::{AgentConfig, RawFrontmatter};
use crate::error::{AgentLoadError, AgentLoadResult};
use crate::parser::{parse_agent, AgentParseError};
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};

/// Stateless loader for parsing and inserting agent configs into [`AgentCatalog`].
///
/// [`AgentLoader`] provides a flexible way to assemble an [`AgentCatalog`] from multiple sources:
/// - Directories (scanned for `agent/**/*.md` and `agents/**/*.md`)
/// - Individual files (names derived from file names, with optional override)
/// - In-memory [`AgentConfig`] entries
///
/// Later insertions override earlier entries with the same name.
///
/// # Example
///
/// ```no_run
/// use llm_coding_tools_agents::{AgentLoader, AgentCatalog};
/// use std::path::Path;
///
/// let mut loader = AgentLoader::new();
/// let mut catalog = AgentCatalog::new();
/// loader.add_directory(&mut catalog, Path::new("~/.opencode"))?;
/// loader.add_file(&mut catalog, Path::new("/path/to/custom_agent.md"))?;
/// # Ok::<(), llm_coding_tools_agents::AgentLoadError>(())
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentLoader;

impl AgentLoader {
    /// Creates a new stateless loader.
    pub fn new() -> Self {
        Self
    }

    /// Adds all agents from a directory to the catalog.
    ///
    /// Scans for `agent/**/*.md` and `agents/**/*.md` patterns. Files that fail
    /// to load are silently skipped. Use [`Self::add_directory_with_errors`] to
    /// receive error callbacks.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert agents into
    /// * `directory` - Root directory to scan
    ///
    /// # Errors
    ///
    /// Returns an error only for directory-level failures (e.g., path is not a directory).
    pub fn add_directory(
        &self,
        catalog: &mut AgentCatalog,
        directory: impl Into<PathBuf>,
    ) -> AgentLoadResult<()> {
        self.add_directory_with_errors(catalog, directory, None::<fn(&Path, &AgentLoadError)>)
    }

    /// Adds all agents from a directory to the catalog with error handling.
    ///
    /// Like [`Self::add_directory`], but invokes the provided callback for each
    /// file that fails to load.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert agents into
    /// * `directory` - Root directory to scan for `agent/**/*.md` and `agents/**/*.md`
    /// * `on_error` - Callback invoked for each file that fails to load
    ///
    /// # Errors
    ///
    /// Returns an error only for directory-level failures (e.g., path is not a directory).
    /// Individual file load failures are reported via `on_error` and do not fail the overall
    /// operation.
    pub fn add_directory_with_errors(
        &self,
        catalog: &mut AgentCatalog,
        directory: impl Into<PathBuf>,
        mut on_error: Option<impl FnMut(&Path, &AgentLoadError)>,
    ) -> AgentLoadResult<()> {
        let dir = directory.into();
        load_directory_with(&dir, |path, name| {
            match load_agent_file(path, name.to_string()) {
                Ok(config) => {
                    catalog.insert(config);
                }
                Err(e) => {
                    if let Some(ref mut handler) = on_error {
                        handler(path, &e);
                    }
                }
            }
            Ok(())
        })
    }

    /// Adds a single agent file (name derived from file name) to the catalog.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert the agent into
    /// * `path` - Path to a markdown file with YAML frontmatter
    pub fn add_file(
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
            return Err(AgentLoadError::schema_validation(
                Some(path.to_path_buf()),
                "agent file name is empty",
            ));
        }
        let config = load_agent_file(&path, derived_name)?;
        catalog.insert(config);
        Ok(())
    }

    /// Adds a single agent file with an explicit name override to the catalog.
    ///
    /// The explicit name always overrides any frontmatter `name` field.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert the agent into
    /// * `path` - Path to a markdown file with YAML frontmatter
    /// * `name` - Explicit agent name to use
    pub fn add_file_named(
        &self,
        catalog: &mut AgentCatalog,
        path: impl Into<PathBuf>,
        name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let path = path.into();
        let override_name = name.into();
        if override_name.is_empty() {
            return Err(AgentLoadError::schema_validation(
                Some(path.to_path_buf()),
                "agent name is empty",
            ));
        }
        let mut config = load_agent_file(&path, String::new())?;
        config.name = override_name;
        catalog.insert(config);
        Ok(())
    }

    /// Adds an in-memory [`AgentConfig`] to the catalog.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert the agent into
    /// * `config` - Fully constructed agent configuration
    pub fn add_config(
        &self,
        catalog: &mut AgentCatalog,
        config: AgentConfig,
    ) -> AgentLoadResult<()> {
        catalog.insert(config);
        Ok(())
    }

    /// Adds an agent configuration from a raw markdown string to the catalog.
    ///
    /// The string should contain YAML frontmatter delimited by `---` followed
    /// by the prompt body. The agent name is derived from the `name` field
    /// in the frontmatter if present; otherwise, `default_name` is used.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert the agent into
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
    /// A convenience wrapper around [`Self::add_from_str`] that converts bytes to UTF-8 string.
    /// Invalid UTF-8 bytes will result in a schema validation error.
    ///
    /// # Arguments
    ///
    /// * `catalog` - The catalog to insert the agent into
    /// * `bytes` - Raw markdown bytes with YAML frontmatter
    /// * `default_name` - Agent name to use if not specified in frontmatter
    pub fn add_from_bytes(
        &self,
        catalog: &mut AgentCatalog,
        bytes: impl AsRef<[u8]>,
        default_name: impl Into<String>,
    ) -> AgentLoadResult<()> {
        let content = std::str::from_utf8(bytes.as_ref()).map_err(|err| {
            AgentLoadError::schema_validation(None, format!("invalid UTF-8: {err}"))
        })?;
        let config = config_from_str_strict(content, default_name)?;
        catalog.insert(config);
        Ok(())
    }
}

/// Shared directory scan helper used by catalog loading.
fn load_directory_with(
    dir: &Path,
    mut on_match: impl FnMut(&Path, &str) -> AgentLoadResult<()>,
) -> AgentLoadResult<()> {
    if !dir.is_dir() {
        if dir.exists() {
            return Err(AgentLoadError::io(
                Some(dir.to_path_buf()),
                std::io::Error::new(std::io::ErrorKind::NotADirectory, "path is not a directory"),
            ));
        }
        // Non-existent directories are allowed (nothing to load)
        return Ok(());
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
            Err(_) => continue, // skip unreadable entries
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

        let name = match derive_agent_name_from_rel(&rel_path) {
            Some(n) => n,
            None => continue,
        };

        on_match(path, name.as_str())?;
    }

    Ok(())
}

/// Shared parse helper that reuses existing loader parsing.
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

fn map_parse_error(path: Option<PathBuf>, err: AgentParseError) -> AgentLoadError {
    match err {
        AgentParseError::SchemaValidation { message } => {
            AgentLoadError::schema_validation(path, message)
        }
        other => AgentLoadError::parse(path, other),
    }
}

/// Loads a single agent configuration from a file.
fn load_agent_file(path: &Path, name: String) -> AgentLoadResult<AgentConfig> {
    let content =
        fs::read_to_string(path).map_err(|e| AgentLoadError::io(Some(path.to_path_buf()), e))?;

    parse_agent_config(content, name).map_err(|err| map_parse_error(Some(path.to_path_buf()), err))
}

/// Strict parser for catalog-only string loading (validates non-empty name).
fn config_from_str_strict(
    markdown: impl Into<String>,
    default_name: impl Into<String>,
) -> AgentLoadResult<AgentConfig> {
    let name = default_name.into();
    let config =
        parse_agent_config(markdown.into(), name).map_err(|err| map_parse_error(None, err))?;
    if config.name.is_empty() {
        return Err(AgentLoadError::schema_validation(
            None,
            "agent name is empty",
        ));
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
    use crate::config::AgentMode;
    use indexmap::IndexMap;
    use indoc::indoc;
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
        assert_eq!(derive_agent_name_from_rel("agent/.md"), None);
        assert_eq!(derive_agent_name_from_rel("agents/.md"), None);
    }

    #[test]
    fn load_agents_derives_name_from_rel_path_not_absolute() {
        // Even if base path contains /agent/, name is derived from rel_path
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test-agent.md",
            indoc! {"
                ---
                mode: subagent
                description: Test
                ---
                Prompt"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        // Name should be "test-agent", not something derived from absolute path
        assert!(catalog.by_name("test-agent").is_some());
    }

    #[test]
    fn load_agents_finds_files_in_agent_dir() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test-agent.md",
            indoc! {"
                ---
                mode: subagent
                description: Test
                ---
                Prompt"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        assert!(catalog.by_name("test-agent").is_some());
        assert_eq!(catalog.by_name("test-agent").unwrap().description, "Test");
        assert_eq!(catalog.by_name("test-agent").unwrap().prompt, "Prompt");
    }

    #[test]
    fn load_agents_finds_files_in_agents_dir() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agents/nested/deep.md",
            indoc! {"
                ---
                mode: primary
                description: Test
                ---
                Body"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        assert!(catalog.by_name("nested/deep").is_some());
    }

    #[test]
    fn load_agents_ignores_non_md_files() {
        let dir = TempDir::new().unwrap();
        create_agent_file(dir.path(), "agent/readme.txt", "not an agent");
        create_agent_file(
            dir.path(),
            "agent/real.md",
            indoc! {"
                ---
                mode: subagent
                description: Test
                ---
                Real"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        assert!(catalog.by_name("real").is_some());
    }

    #[test]
    fn load_agents_ignores_files_outside_agent_dirs() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "other/file.md",
            indoc! {"
                ---
                mode: subagent
                ---
                Body"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        assert!(catalog.iter().count() == 0);
    }

    #[test]
    fn load_agents_scans_multiple_directories() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        create_agent_file(
            dir1.path(),
            "agent/first.md",
            indoc! {"
                ---
                mode: subagent
                description: First
                ---"
            },
        );
        create_agent_file(
            dir2.path(),
            "agent/second.md",
            indoc! {"
                ---
                mode: primary
                description: Second
                ---"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir1.path()).unwrap();
        loader.add_directory(&mut catalog, dir2.path()).unwrap();

        assert!(catalog.by_name("first").is_some());
        assert!(catalog.by_name("second").is_some());
    }

    #[test]
    fn load_agents_handles_model_with_colons() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/test.md",
            indoc! {"
                ---
                model: provider/model:tag
                mode: subagent
                description: Test
                ---
                Body"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        assert_eq!(
            catalog.by_name("test").unwrap().model,
            Some("provider/model:tag".to_string())
        );
    }

    #[test]
    fn load_agents_parses_permissions() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/perms.md",
            indoc! {"
                ---
                mode: subagent
                description: Test
                permission:
                  bash: allow
                  task: deny
                ---"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();
        let perms = &catalog.by_name("perms").unwrap().permission;

        assert_eq!(perms.len(), 2);
    }

    #[test]
    fn load_agents_handles_flow_permission_syntax() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/flow.md",
            indoc! {r#"
                ---
                mode: subagent
                description: Test
                permission:
                  task: { "*": "deny" }
                ---"#
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();
        // Should parse without error (flow syntax preserved)
        assert!(catalog.by_name("flow").is_some());
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
                indoc! {"
                    ---
                    mode: subagent
                    description: Test
                    ---
                    Body"
                },
                None,
                "example",
            ),
            (
                "custom/agent.md",
                indoc! {"
                    ---
                    mode: subagent
                    description: Test
                    ---
                    Body"
                },
                Some("override/name"),
                "override/name",
            ),
            (
                "custom/agent.md",
                indoc! {"
                    ---
                    name: frontmatter-name
                    mode: subagent
                    description: Test
                    ---
                    Body            "},
                Some("override/name"),
                "override/name",
            ),
        ];

        for (rel_path, content, override_name, expected) in cases {
            let dir = TempDir::new().unwrap();
            create_agent_file(dir.path(), rel_path, content);

            let loader = AgentLoader::new();
            let mut catalog = AgentCatalog::new();
            let full_path = dir.path().join(rel_path);
            match override_name {
                Some(name) => {
                    loader
                        .add_file_named(&mut catalog, full_path, name)
                        .unwrap();
                }
                None => {
                    loader.add_file(&mut catalog, full_path).unwrap();
                }
            }

            assert!(catalog.by_name(expected).is_some());
        }
    }

    #[test]
    fn agent_loader_allows_in_memory_config_and_overrides() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "custom/agent.md",
            indoc! {"
                ---
                mode: subagent
                description: First
                ---
                Body"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader
            .add_file(&mut catalog, dir.path().join("custom/agent.md"))
            .unwrap();
        loader
            .add_config(&mut catalog, make_agent("agent", "Second"))
            .unwrap();

        assert_eq!(catalog.by_name("agent").unwrap().description, "Second");
    }

    #[test]
    fn agent_loader_loads_into_existing_catalog() {
        let mut catalog = AgentCatalog::new();
        catalog.insert(make_agent("existing", "keep"));

        let loader = AgentLoader::new();
        loader
            .add_config(&mut catalog, make_agent("new", "added"))
            .unwrap();

        assert!(catalog.by_name("existing").is_some());
        assert!(catalog.by_name("new").is_some());
    }

    #[test]
    fn agent_loader_loads_explicit_file_without_agent_prefix() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "custom/explicit.md",
            indoc! {"
                ---
                mode: subagent
                description: Explicit
                ---
                Body"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader
            .add_file(&mut catalog, dir.path().join("custom/explicit.md"))
            .unwrap();

        let agent = catalog.by_name("explicit").unwrap();
        assert_eq!(agent.description, "Explicit");
    }

    #[test]
    fn agent_loader_scans_directories_with_agent_patterns() {
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/one.md",
            indoc! {"
                ---
                mode: subagent
                description: First agent
                ---
                One"
            },
        );
        create_agent_file(
            dir.path(),
            "agents/nested/two.md",
            indoc! {"
                ---
                mode: primary
                description: Second agent
                ---
                Two"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        assert!(catalog.by_name("one").is_some());
        assert!(catalog.by_name("nested/two").is_some());
    }

    #[test]
    fn agent_loader_overrides_existing_catalog_entries() {
        // Later insertions (from the loader) override earlier catalog entries with the same name.
        let mut catalog = AgentCatalog::new();
        catalog.insert(make_agent("override", "old"));

        let loader = AgentLoader::new();
        loader
            .add_config(&mut catalog, make_agent("override", "new"))
            .unwrap();

        assert_eq!(catalog.by_name("override").unwrap().description, "new");
    }

    // ========== String/Bytes Tests ==========

    #[test]
    fn catalog_add_from_str_uses_default_name_when_missing() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let markdown = indoc! {"
            ---
            mode: subagent
            description: From string
            ---
            Body"
        };

        loader
            .add_from_str(&mut catalog, markdown, "string-agent")
            .unwrap();

        let agent = catalog.by_name("string-agent").unwrap();
        assert_eq!(agent.description, "From string");
    }

    #[test]
    fn catalog_add_from_str_uses_frontmatter_name() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let markdown = indoc! {"
            ---
            name: frontmatter-name
            mode: subagent
            description: Test
            ---
            Body"
        };

        loader
            .add_from_str(&mut catalog, markdown, "default-name")
            .unwrap();

        assert!(catalog.by_name("frontmatter-name").is_some());
        assert!(catalog.by_name("default-name").is_none());
    }

    #[test]
    fn catalog_add_from_str_errors_on_empty_name() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let markdown = indoc! {"
            ---
            mode: subagent
            description: Test
            ---
            Body"
        };
        let result = loader.add_from_str(&mut catalog, markdown, "");

        assert!(matches!(
            result,
            Err(AgentLoadError::SchemaValidation { .. })
        ));
    }

    #[test]
    fn catalog_add_from_bytes_validates_utf8() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let bytes = indoc! {b"
            ---
            name: test
            mode: subagent
            description: Test
            ---
            Body"};

        loader.add_from_bytes(&mut catalog, bytes, "test").unwrap();

        assert!(catalog.by_name("test").is_some());
    }

    #[test]
    fn catalog_add_from_bytes_errors_on_invalid_utf8() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let bytes: &[u8] = &[0xFF, 0xFE, 0xFD]; // Invalid UTF-8

        let result = loader.add_from_bytes(&mut catalog, bytes, "test");

        assert!(matches!(
            result,
            Err(AgentLoadError::SchemaValidation { .. })
        ));
    }

    #[test]
    fn load_agents_skips_files_with_missing_description() {
        // Directory loading skips files missing required description field
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/no-desc.md",
            indoc! {"
                ---
                mode: subagent
                ---
                Prompt without description"
            },
        );
        // Add a valid file to ensure directory load succeeds
        create_agent_file(
            dir.path(),
            "agent/valid.md",
            indoc! {"
                ---
                mode: subagent
                description: Valid agent
                ---
                Valid prompt"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        // Invalid file should be skipped
        assert!(catalog.by_name("no-desc").is_none());
        // Valid file should be loaded
        assert!(catalog.by_name("valid").is_some());
    }

    #[test]
    fn load_agents_succeeds_with_missing_mode() {
        // Mode defaults to All when not provided
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/no-mode.md",
            indoc! {"
                ---
                description: Test agent
                ---
                Prompt without mode"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        let agent = catalog.by_name("no-mode").unwrap();
        assert_eq!(agent.mode, AgentMode::All);
        assert_eq!(agent.description, "Test agent");
    }

    #[test]
    fn load_agents_skips_files_with_invalid_mode() {
        // Directory loading skips files with invalid mode values
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/invalid-mode.md",
            indoc! {"
                ---
                mode: invalid_mode
                description: Test agent
                ---
                Prompt with invalid mode"
            },
        );
        // Add a valid file to ensure directory load succeeds
        create_agent_file(
            dir.path(),
            "agent/valid.md",
            indoc! {"
                ---
                mode: subagent
                description: Valid agent
                ---
                Valid prompt"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        loader.add_directory(&mut catalog, dir.path()).unwrap();

        // Invalid file should be skipped
        assert!(catalog.by_name("invalid-mode").is_none());
        // Valid file should be loaded
        assert!(catalog.by_name("valid").is_some());
    }

    #[test]
    fn add_file_errors_on_missing_description() {
        // Single file loading fails when description is missing
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/no-desc.md",
            indoc! {"
                ---
                mode: subagent
                ---
                Prompt without description"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let result = loader.add_file(&mut catalog, dir.path().join("agent/no-desc.md"));

        assert!(result.is_err());
        assert!(catalog.by_name("no-desc").is_none());
    }

    #[test]
    fn add_file_errors_on_invalid_mode() {
        // Single file loading fails with invalid mode
        let dir = TempDir::new().unwrap();
        create_agent_file(
            dir.path(),
            "agent/invalid-mode.md",
            indoc! {"
                ---
                mode: invalid_mode
                description: Test agent
                ---
                Prompt with invalid mode"
            },
        );

        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let result = loader.add_file(&mut catalog, dir.path().join("agent/invalid-mode.md"));

        assert!(result.is_err());
        assert!(catalog.by_name("invalid-mode").is_none());
    }

    #[test]
    fn add_file_rejects_permission_task_ask_scalar() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let markdown = indoc! {"
            ---
            description: Ask scalar
            permission:
              task: ask
            ---
            Prompt"
        };

        let result = loader.add_from_str(&mut catalog, markdown, "ask-scalar");
        assert!(matches!(
            result,
            Err(AgentLoadError::SchemaValidation { message, .. })
                if message.contains("permission.task: ask is unsupported")
        ));
    }

    #[test]
    fn add_file_rejects_permission_task_ask_pattern_map() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let markdown = indoc! {r#"
            ---
            description: Ask map
            permission:
              task:
                "*": ask
            ---
            Prompt"#
        };

        let result = loader.add_from_str(&mut catalog, markdown, "ask-map");
        assert!(matches!(
            result,
            Err(AgentLoadError::SchemaValidation { message, .. })
                if message.contains("permission.task: ask is unsupported")
        ));
    }

    #[test]
    fn add_from_str_accepts_hidden_true() {
        let loader = AgentLoader::new();
        let mut catalog = AgentCatalog::new();
        let markdown = indoc! {"
            ---
            description: Hidden agent
            hidden: true
            ---
            Prompt"
        };

        loader
            .add_from_str(&mut catalog, markdown, "hidden-agent")
            .unwrap();
        let agent = catalog.by_name("hidden-agent").unwrap();
        assert!(agent.hidden);
        assert_eq!(agent.description, "Hidden agent");
    }
}
