//! File-tool path resolver construction.
//!
//! [`FileToolResolver`] is a closed enum of resolver types that avoids
//! `Box<dyn PathResolver>`. [`build_resolver_for_tool`] inspects permission
//! config and returns the cheapest resolver that satisfies it.
//!
//! # Optimisation tiers
//!
//! | Config pattern                     | Resolver                      | Cost             |
//! |------------------------------------|-------------------------------|------------------|
//! | No config for tool                 | `AllowedPathResolver([root])` | prefix check     |
//! | `Action(Allow)`                    | `AllowedPathResolver([root])` | prefix check     |
//! | Pattern `**` with Allow            | `AllowedPathResolver([root])` | prefix check     |
//! | `/**` with Allow                   | `AbsolutePathResolver`        | zero             |
//! | Otherwise                          | `AllowedGlobResolver`         | glob matching    |

use crate::types::PermissionRule;
use indexmap::IndexMap;
use llm_coding_tools_core::context::PathMode;
use llm_coding_tools_core::error::{ToolError, ToolResult};
use llm_coding_tools_core::path::{
    expand_shell, AbsolutePathResolver, AllowedGlobResolver, AllowedPathResolver, GlobPolicy,
    PathResolver, RuleAction,
};
use llm_coding_tools_core::permissions::PermissionAction;
use soft_canonicalize::soft_canonicalize;
use std::path::{Path, PathBuf};

/// Closed enum of resolver types used for file tools.
///
/// Avoids [`Box<dyn PathResolver>`] which cannot implement `Clone`.
/// All tool wrappers require `R: PathResolver + Clone`.
#[derive(Debug, Clone)]
pub enum FileToolResolver {
    /// Unrestricted: any absolute path is allowed.
    Absolute(AbsolutePathResolver),
    /// Prefix-check: paths must start with one of the allowed directories.
    Allowed(AllowedPathResolver),
    /// Glob-filtered: paths must match configured glob patterns.
    Glob(AllowedGlobResolver),
}

impl PathResolver for FileToolResolver {
    fn resolve(&self, path: &str) -> ToolResult<PathBuf> {
        match self {
            Self::Absolute(r) => r.resolve(path),
            Self::Allowed(r) => r.resolve(path),
            Self::Glob(r) => r.resolve(path),
        }
    }

    fn is_path_allowed(&self, path: &Path) -> bool {
        match self {
            Self::Absolute(r) => r.is_path_allowed(path),
            Self::Allowed(r) => r.is_path_allowed(path),
            Self::Glob(r) => r.is_path_allowed(path),
        }
    }

    fn path_mode(&self) -> PathMode {
        match self {
            Self::Absolute(_) => PathMode::Absolute,
            Self::Allowed(_) | Self::Glob(_) => PathMode::Allowed,
        }
    }
}

/// Builds the cheapest resolver that satisfies the permission config.
///
/// # Optimisation tiers
///
/// - No config for tool -> `AllowedPathResolver([workspace_root])` (workspace only)
/// - `Action(Allow)` -> `AllowedPathResolver([workspace_root])`
/// - Pattern `"**"` with Allow -> `AllowedPathResolver([workspace_root])` (workspace only)
/// - `"/**"` -> `AbsolutePathResolver` (any absolute path)
/// - Otherwise -> `AllowedGlobResolver` with `GlobPolicy`
///
/// # Arguments
///
/// - `config` - Permission config mapping tool names to [`PermissionRule`].
/// - `tool_name` - Name of the tool to look up in `config`.
/// - `workspace_root` - Workspace root used for relative-pattern resolution.
///
/// # Returns
///
/// The cheapest [`FileToolResolver`] variant satisfying the tool's permission config.
///
/// # Errors
///
/// - Returns [`ToolError::InvalidPath`] when shell expansion fails (e.g., unresolvable `$VAR` in a pattern).
/// - Returns [`ToolError::InvalidPattern`] when a glob pattern is syntactically invalid.
/// - Returns [`ToolError::InvalidPath`] when the workspace root does not exist or cannot be canonicalized.
pub fn build_resolver_for_tool(
    config: &IndexMap<String, PermissionRule>,
    tool_name: &str,
    workspace_root: &Path,
) -> Result<FileToolResolver, ToolError> {
    let workspace_root = soft_canonicalize(workspace_root).map_err(|e| {
        ToolError::InvalidPath(format!(
            "failed to resolve workspace root '{}': {}",
            workspace_root.display(),
            e
        ))
    })?;

    let Some(rule) = config.get(tool_name) else {
        // Nothing specified: default to workspace only.
        let resolver = AllowedPathResolver::from_canonical(vec![workspace_root]);
        return Ok(FileToolResolver::Allowed(resolver));
    };
    match rule {
        PermissionRule::Action(PermissionAction::Deny) => Err(ToolError::PermissionDenied {
            tool: "file",
            subject: format!("tool '{}' is disabled by configuration", tool_name),
        }),
        PermissionRule::Action(PermissionAction::Allow) => {
            // Action(Allow): workspace only.
            let resolver = AllowedPathResolver::from_canonical(vec![workspace_root]);
            Ok(FileToolResolver::Allowed(resolver))
        }
        PermissionRule::Pattern(patterns) => {
            // Optimisation: all-allow patterns
            if patterns.values().all(|a| *a == PermissionAction::Allow) {
                // "/**" -> unrestricted access to any absolute path
                if let Some(resolver) = try_globstar_optimisation(patterns)? {
                    return Ok(FileToolResolver::Absolute(resolver));
                }
                // "**" -> workspace only (equivalent to Action(Allow))
                if is_bare_globstar(patterns) {
                    let resolver = AllowedPathResolver::from_canonical(vec![workspace_root]);
                    return Ok(FileToolResolver::Allowed(resolver));
                }
            }
            // Fall through to full glob policy
            let resolver = AllowedGlobResolver::from_canonical(&workspace_root)
                .with_policy(build_glob_policy(patterns, &workspace_root)?);
            Ok(FileToolResolver::Glob(resolver))
        }
    }
}

/// Checks if any pattern is `/**` (unrestricted access to all absolute paths).
///
/// Returns `Some(AbsolutePathResolver)` if found, `None` otherwise.
fn try_globstar_optimisation(
    patterns: &IndexMap<String, PermissionAction>,
) -> Result<Option<AbsolutePathResolver>, ToolError> {
    for pattern in patterns.keys() {
        let expanded = expand_shell(pattern)?;
        if expanded.to_string_lossy() == "/**" {
            return Ok(Some(AbsolutePathResolver));
        }
    }
    Ok(None)
}

/// Checks if the pattern map contains exactly one pattern "**" (bare globstar).
///
/// Returns `true` if there's a single pattern and it expands to "**",
/// indicating workspace-only access (equivalent to `Action(Allow)`).
fn is_bare_globstar(patterns: &IndexMap<String, PermissionAction>) -> bool {
    if patterns.len() != 1 {
        return false;
    }
    let pattern = patterns.keys().next().expect("len == 1");
    if let Ok(expanded) = expand_shell(pattern) {
        return expanded.to_string_lossy() == "**";
    }
    false
}

/// Builds a `GlobPolicy` from a pattern map.
fn build_glob_policy(
    patterns: &IndexMap<String, PermissionAction>,
    workspace_root: &Path,
) -> Result<GlobPolicy, ToolError> {
    let mut builder = GlobPolicy::builder_with_base(workspace_root)?;
    for (pattern, action) in patterns {
        let rule_action = match action {
            PermissionAction::Allow => RuleAction::Allow,
            PermissionAction::Deny => RuleAction::Deny,
        };
        builder = builder.add(pattern, rule_action)?;
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use soft_canonicalize::soft_canonicalize;

    type TestResult = Result<(), ToolError>;

    // ---------------------------------------------------------------
    // build_resolver_for_tool: no config for tool
    // Default: workspace only (AllowedPathResolver).
    // ---------------------------------------------------------------

    #[test]
    fn no_config_returns_allowed() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();

        let config = IndexMap::new();
        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        let FileToolResolver::Allowed(inner) = &resolver else {
            panic!("expected Allowed, got {resolver:?}");
        };

        // Allowed paths should be exactly [workspace_root].
        let expected = soft_canonicalize(temp.path())?;
        assert_eq!(inner.allowed_paths(), &[expected]);

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: Action(Allow)
    // Scalar allow -> workspace only (AllowedPathResolver).
    // ---------------------------------------------------------------

    #[test]
    fn action_allow_returns_allowed() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();

        let mut config = IndexMap::new();
        config.insert(
            "read".to_string(),
            PermissionRule::Action(PermissionAction::Allow),
        );

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        let FileToolResolver::Allowed(inner) = &resolver else {
            panic!("expected Allowed, got {resolver:?}");
        };

        // Allowed paths should be exactly [workspace_root].
        let expected = soft_canonicalize(temp.path())?;
        assert_eq!(inner.allowed_paths(), &[expected]);

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: pattern "/**"
    // Unrestricted: any absolute path is allowed (AbsolutePathResolver).
    // ---------------------------------------------------------------

    #[test]
    fn absolute_globstar_returns_absolute() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("/**".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        // Any absolute path is allowed, even outside the workspace.
        assert!(resolver.is_path_allowed(Path::new("/etc/passwd")));

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: pattern "**" (bare globstar)
    // Equivalent to Action(Allow): workspace only via AllowedPathResolver.
    // ---------------------------------------------------------------

    #[test]
    fn bare_globstar_returns_allowed_workspace_root() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("**".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        let FileToolResolver::Allowed(inner) = &resolver else {
            panic!("expected Allowed, got {resolver:?}");
        };

        // Bare "**" -> workspace root (same as Action(Allow)).
        let expected = soft_canonicalize(temp.path())?;
        assert_eq!(inner.allowed_paths(), &[expected.clone()]);

        // Any path within workspace should be allowed.
        assert!(
            resolver.is_path_allowed(&expected.join("src/lib.rs")),
            "** should permit src/lib.rs"
        );
        assert!(
            resolver.is_path_allowed(&expected.join("any/path/file.txt")),
            "** should permit any path"
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: pattern "src/**"
    // Prefix patterns now use AllowedGlobResolver (prefix-globstar
    // optimisation was removed to fix workspace-relative resolution).
    // ---------------------------------------------------------------

    #[test]
    fn prefix_globstar_returns_glob() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("src/**".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        assert!(
            matches!(resolver, FileToolResolver::Glob(_)),
            "prefix patterns should use AllowedGlobResolver"
        );

        let root = soft_canonicalize(temp.path())?;

        // src/** allow should permit src/lib.rs.
        assert!(
            resolver.is_path_allowed(&root.join("src/lib.rs")),
            "src/** should permit src/lib.rs"
        );

        // Paths outside src/ should be denied.
        assert!(
            !resolver.is_path_allowed(&root.join("tests/lib.rs")),
            "paths outside src/ should be denied"
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: mixed allow + deny patterns
    // Cannot optimise; falls through to GlobResolver.
    // ---------------------------------------------------------------

    #[test]
    fn mixed_allow_deny_returns_glob() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("src/**".to_string(), PermissionAction::Allow);
        patterns.insert("*.secret".to_string(), PermissionAction::Deny);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        assert!(matches!(resolver, FileToolResolver::Glob(_)));

        let root = soft_canonicalize(temp.path())?;

        // *.secret deny should block test.secret.
        let secret_path = root.join("test.secret");
        assert!(
            !resolver.is_path_allowed(secret_path.as_path()),
            "*.secret deny should block test.secret"
        );

        // src/** allow should permit src/lib.rs.
        let src_path = root.join("src/lib.rs");
        assert!(
            resolver.is_path_allowed(src_path.as_path()),
            "src/** allow should permit src/lib.rs"
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: non-** glob pattern ("**/*.rs")
    // Not a simple prefix glob; falls through to GlobResolver.
    // ---------------------------------------------------------------

    #[test]
    fn non_globstar_pattern_returns_glob() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("**/*.rs".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        assert!(matches!(resolver, FileToolResolver::Glob(_)));

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: multiple <prefix>/** patterns
    // Prefix patterns now use AllowedGlobResolver.
    // ---------------------------------------------------------------

    #[test]
    fn multiple_prefix_globstars_return_glob() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::create_dir_all(temp.path().join("tests")).unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("src/**".to_string(), PermissionAction::Allow);
        patterns.insert("tests/**".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        assert!(
            matches!(resolver, FileToolResolver::Glob(_)),
            "multiple prefix globs should use AllowedGlobResolver"
        );

        let root = soft_canonicalize(temp.path())?;

        // src/** allow should permit src/lib.rs.
        assert!(
            resolver.is_path_allowed(&root.join("src/lib.rs")),
            "src/** should permit src/lib.rs"
        );

        // tests/** allow should permit tests/lib.rs.
        assert!(
            resolver.is_path_allowed(&root.join("tests/lib.rs")),
            "tests/** should permit tests/lib.rs"
        );

        // Paths outside both src/ and tests/ should be denied.
        assert!(
            !resolver.is_path_allowed(&root.join("docs/lib.rs")),
            "paths outside src/ and tests/ should be denied"
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: empty pattern map
    // No patterns to optimise; falls through to GlobResolver.
    // ---------------------------------------------------------------

    #[test]
    fn empty_pattern_map_returns_glob() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();

        let patterns = IndexMap::new();
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;

        assert!(matches!(resolver, FileToolResolver::Glob(_)));

        Ok(())
    }

    // ---------------------------------------------------------------
    // build_resolver_for_tool: invalid shell variable
    // Shell expansion fails; should return an error.
    // ---------------------------------------------------------------

    #[test]
    fn invalid_shell_variable_should_return_error() {
        let temp = tempfile::TempDir::new().unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert(
            "$DEFINITELY_NOT_SET_12345/**".to_string(),
            PermissionAction::Allow,
        );
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let result = build_resolver_for_tool(&config, "read", temp.path());
        assert!(
            result.is_err(),
            "unresolvable shell variable should produce an error"
        );
    }

    // ---------------------------------------------------------------
    // Regression: prefix-globstar optimisation breaks
    // workspace-relative resolution for non-traversal tools.
    //
    // Pattern "src/**" creates an AllowedPathResolver whose only
    // allowed base is workspace_root/src.  When resolve() is called
    // with "src/lib.rs" the resolver does base.join("src/lib.rs")
    // which yields workspace_root/src/src/lib.rs — a doubled "src".
    // ---------------------------------------------------------------

    #[test]
    fn prefix_globstar_doubles_relative_prefix_for_non_traversal() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/lib.rs"), "").unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("src/**".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;
        let root = soft_canonicalize(temp.path())?;

        let resolved = resolver.resolve("src/lib.rs")?;

        assert_eq!(
            resolved,
            root.join("src/lib.rs"),
            "resolve(\"src/lib.rs\") should be workspace_root/src/lib.rs, got {:?}",
            resolved
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    // Regression: prefix-globstar optimisation is removed.
    // All tools now use AllowedGlobResolver which correctly resolves
    // "." to the workspace root (not a subdirectory).
    // ---------------------------------------------------------------

    #[test]
    fn prefix_patterns_resolve_dot_to_workspace_root() -> TestResult {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::create_dir_all(temp.path().join("tests")).unwrap();

        let mut patterns = IndexMap::new();
        patterns.insert("src/**".to_string(), PermissionAction::Allow);
        patterns.insert("tests/**".to_string(), PermissionAction::Allow);
        let mut config = IndexMap::new();
        config.insert("read".to_string(), PermissionRule::Pattern(patterns));

        let resolver = build_resolver_for_tool(&config, "read", temp.path())?;
        let workspace_root = soft_canonicalize(temp.path())?;
        let resolved = resolver.resolve(".")?;

        assert_eq!(
            resolved, workspace_root,
            "resolve('.') must return workspace root, got {:?}",
            resolved
        );

        Ok(())
    }
}
