//! Glob pattern file matching operation.

use crate::error::{ToolError, ToolResult};
use crate::path::allowed_glob::normalize::normalize_path;
use crate::path::PathResolver;
use crate::tool_metadata::glob as glob_meta;
use globset::Glob;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;

/// Serde-friendly glob request owned by the core crate.
#[derive(Debug, Deserialize)]
pub struct GlobRequest {
    /// Glob pattern to match against file paths (e.g. `"**/*.rs"`).
    pub pattern: String,
    /// Absolute or relative directory path to search.
    pub path: String,
}

impl GlobRequest {
    /// Parses a raw JSON tool payload into a glob request.
    ///
    /// # Errors
    /// - Returns [`ToolError::Json`] when the JSON payload cannot be deserialized
    ///   into a [`GlobRequest`] (e.g., missing required `pattern` or `path` fields,
    ///   or invalid field types).
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to glob requests.
///
/// The `limit` field caps the number of file paths returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobSettings {
    /// Maximum number of file paths to return.
    limit: usize,
}

impl Default for GlobSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobSettings {
    /// Creates valid glob settings with the standard result limit.
    #[must_use]
    pub fn new() -> Self {
        Self {
            limit: glob_meta::MAX_RESULTS,
        }
    }

    /// Updates the maximum number of files returned.
    ///
    /// # Errors
    /// - Returns an error when `limit` is below [`MIN_LIMIT`].
    ///
    /// [`MIN_LIMIT`]: crate::util::MIN_LIMIT
    pub fn with_limit(mut self, limit: usize) -> ToolResult<Self> {
        use crate::util::MIN_LIMIT;
        if limit < MIN_LIMIT {
            return Err(ToolError::validation_for(
                "limit",
                format!("limit must be >= {}", MIN_LIMIT),
            ));
        }
        self.limit = limit;
        Ok(self)
    }

    /// Returns the maximum number of files to return.
    ///
    /// # Returns
    /// - The configured result limit.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }
}

/// Output from glob file matching.
#[derive(Debug, Serialize)]
pub struct GlobOutput {
    /// Matched file paths relative to search directory, sorted by mtime (newest first).
    pub files: Vec<String>,
    /// Whether results were truncated due to limit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
    /// Whether one or more paths could not be traversed or processed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// Per-path traversal errors encountered while collecting matches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Finds files matching a glob pattern in the given directory.
///
/// Results are sorted by modification time (newest first) and respect `.gitignore`.
/// The `limit` parameter controls the maximum number of files returned.
///
/// # Arguments
/// - `resolver` - [`PathResolver`] used to resolve `request.path` to an absolute directory
///   and to filter walked entries via [`PathResolver::is_path_allowed`].
/// - `request` - The glob request containing the pattern and search directory.
/// - `settings` - Runtime settings including result limit.
///
/// # Returns
/// - [`GlobOutput`] with matched file paths, sorted newest-first, plus truncation and
///   error metadata.
///
/// # Errors
/// - Returns [`ToolError::InvalidPath`] when the resolved path is not a directory.
/// - Returns [`ToolError::InvalidPattern`] when the glob pattern fails to compile.
/// - Returns [`ToolError::InvalidPath`] when path resolution fails.
pub fn glob_files<R: PathResolver>(
    resolver: &R,
    request: GlobRequest,
    settings: &GlobSettings,
) -> ToolResult<GlobOutput> {
    // Resolve the requested path to an absolute directory.
    let path = resolver.resolve(&request.path)?;

    if !path.is_dir() {
        return Err(ToolError::InvalidPath(format!(
            "path is not a directory: {}",
            path.display()
        )));
    }

    let limit = settings.limit();

    // Compile the glob pattern once for repeated matching.
    let matcher = Glob::new(&request.pattern)?.compile_matcher();

    let mut entries: Vec<(std::path::PathBuf, SystemTime)> = Vec::new();
    let mut errors: Vec<String> = Vec::with_capacity(8);

    // Walk the directory tree, honouring .gitignore at every level.
    let walker = WalkBuilder::new(&path)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry_result in walker {
        // Record traversal errors but keep walking.
        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                errors.push(format!("walk error: {err}"));
                continue;
            }
        };

        // Skip directories — we only want files.
        if let Some(ft) = entry.file_type() {
            if ft.is_dir() {
                continue;
            }
        } else {
            continue;
        }

        // Strip the search-root prefix to get a relative path.
        let rel_path = match entry.path().strip_prefix(&path) {
            Ok(p) => p,
            Err(err) => {
                errors.push(format!(
                    "failed to make '{}' relative to '{}': {err}",
                    entry.path().display(),
                    path.display()
                ));
                continue;
            }
        };

        // When WalkBuilder yields the search root itself, strip_prefix
        // produces an empty path - skip it so we only match against files.
        if rel_path.as_os_str().is_empty() {
            continue;
        }

        // Normalise separators to forward slashes.
        let rel_str = normalize_path(rel_path);

        // Skip files that don't match the glob pattern.
        if !matcher.is_match(rel_str.as_ref()) {
            continue;
        }

        // Drop files the resolver doesn't allow.
        if !resolver.is_path_allowed(entry.path()) {
            continue;
        }

        // Read mtime, falling back to epoch when unavailable.
        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        entries.push((rel_path.to_path_buf(), mtime));
    }

    let total = entries.len();
    let truncated = total > limit;

    // Sort newest-first; break ties by path. Partial-sort when over limit
    // to avoid ordering entries that will be discarded.
    if total <= limit {
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    } else {
        entries.select_nth_unstable_by(limit - 1, |a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        entries[..limit].sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }

    // Build the final list of normalised relative paths.
    let files: Vec<String> = entries[..limit.min(total)]
        .iter()
        .map(|(p, _)| normalize_path(p).into_owned())
        .collect();

    Ok(GlobOutput {
        files,
        truncated,
        partial: !errors.is_empty(),
        errors,
    })
}

#[cfg(test)]
mod tests {
    //! Tests for the [`glob_files`] operation covering pattern matching, sorting,
    //! gitignore handling, limit enforcement, and serialization.
    use super::*;
    use crate::path::AbsolutePathResolver;
    use crate::path::AllowedGlobResolver;
    use crate::path::GlobPolicy;
    use rstest::rstest;
    use soft_canonicalize::soft_canonicalize;
    use std::fs::{self, File, FileTimes};
    use std::io::Write;
    use std::path::Path;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn test_settings() -> GlobSettings {
        GlobSettings::new().with_limit(1000).unwrap()
    }

    fn test_settings_with_limit(limit: usize) -> GlobSettings {
        GlobSettings::new().with_limit(limit).unwrap()
    }

    fn run_glob(path: &Path, pattern: &str) -> GlobOutput {
        run_glob_with_settings(path, pattern, &test_settings())
    }

    fn run_glob_with_limit(path: &Path, pattern: &str, limit: usize) -> GlobOutput {
        run_glob_with_settings(path, pattern, &test_settings_with_limit(limit))
    }

    fn run_glob_with_settings(path: &Path, pattern: &str, settings: &GlobSettings) -> GlobOutput {
        let resolver = AbsolutePathResolver;
        glob_files(
            &resolver,
            GlobRequest {
                pattern: pattern.to_string(),
                path: path.to_str().unwrap().to_string(),
            },
            settings,
        )
        .unwrap()
    }

    /// Creates a temporary directory tree with `.git`, `src/lib.rs`, `Cargo.toml`,
    /// and a `target/` directory excluded via `.gitignore`.
    fn create_test_tree() -> TempDir {
        let dir = TempDir::new().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join(".git")).unwrap();
        fs::create_dir_all(base.join("src")).unwrap();
        File::create(base.join("src/lib.rs")).unwrap();
        File::create(base.join("Cargo.toml")).unwrap();
        fs::create_dir_all(base.join("target")).unwrap();
        File::create(base.join("target/binary")).unwrap();
        let mut gitignore = File::create(base.join(".gitignore")).unwrap();
        writeln!(gitignore, "target/").unwrap();
        dir
    }

    #[test]
    fn glob_settings_should_create_standard_defaults() {
        let settings = GlobSettings::new();
        assert_eq!(settings.limit(), glob_meta::MAX_RESULTS);
    }

    #[test]
    fn glob_settings_should_reject_zero_limit() {
        assert!(GlobSettings::new().with_limit(0).is_err());
    }

    /// Verifies that glob patterns correctly include or exclude files based on
    /// both pattern matching and gitignore rules.
    #[rstest]
    #[case::matches_rs_extension("**/*.rs", "lib.rs", true)]
    #[case::excludes_gitignored_target("**/*", "target", false)]
    fn glob_pattern_includes_or_excludes_files(
        #[case] pattern: &str,
        #[case] needle: &str,
        #[case] should_find: bool,
    ) {
        let dir = create_test_tree();
        let result = run_glob(dir.path(), pattern);

        let found = result.files.iter().any(|f| f.contains(needle));

        assert_eq!(
            found, should_find,
            "pattern={pattern}, needle={needle}, files={:?}",
            result.files
        );
    }

    /// Verifies that optional JSON fields are only serialized when they contain
    /// meaningful data. GlobOutput uses `#[serde(skip_serializing_if)]` to omit
    /// `partial` when false and `errors` when empty, producing cleaner JSON output.
    ///
    /// Test matrix:
    /// - Case 1: partial=true, has errors → both fields appear in JSON
    /// - Case 2: partial=false, no errors → neither field appears in JSON
    ///
    /// We verify this behaviour specifically to ensure the LLM does not receive
    /// unnecessary tokens for default values that provide no information.
    #[rstest]
    #[case::partial_with_errors(true, vec!["walk error: permission denied"])]
    #[case::clean_results_no_optional_fields(false, vec![])]
    fn glob_output_serialization_omits_default_fields(
        #[case] partial: bool,     // Whether walk encountered errors
        #[case] errors: Vec<&str>, // Error messages from walk
    ) {
        // Save error count before consuming the vec for assertions later
        let error_count = errors.len();

        // Compute expected field presence from input values
        let expected_partial_present = partial;
        let expected_errors_present = !errors.is_empty();

        let output = GlobOutput {
            files: vec!["src/lib.rs".to_string()],
            truncated: false,
            partial,
            errors: errors.into_iter().map(String::from).collect(),
        };

        let json = serde_json::to_value(&output).unwrap();

        // Verify presence/absence of optional fields based on their values
        assert_eq!(
            json.get("partial").is_some(),
            expected_partial_present,
            "partial field presence mismatch in JSON: {json}"
        );
        assert_eq!(
            json.get("errors").is_some(),
            expected_errors_present,
            "errors field presence mismatch in JSON: {json}"
        );

        // When errors are present, verify they serialized correctly
        if expected_errors_present {
            let error_values = json.get("errors").unwrap().as_array().unwrap();
            assert_eq!(error_values.len(), error_count);
        }
    }

    #[test]
    fn glob_sorts_by_mtime_desc() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        let older_path = base.join("older.txt");
        let newer_path = base.join("newer.txt");
        let older_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let newer_time = SystemTime::UNIX_EPOCH + Duration::from_secs(2);

        let older_file = File::create(&older_path).unwrap();
        older_file
            .set_times(FileTimes::new().set_modified(older_time))
            .unwrap();
        let newer_file = File::create(&newer_path).unwrap();
        newer_file
            .set_times(FileTimes::new().set_modified(newer_time))
            .unwrap();

        let result = run_glob(base, "**/*.txt");

        let newer_index = result
            .files
            .iter()
            .position(|path| path.ends_with("newer.txt"))
            .unwrap();
        let older_index = result
            .files
            .iter()
            .position(|path| path.ends_with("older.txt"))
            .unwrap();

        assert!(
            newer_index < older_index,
            "expected newer file before older: {:?}",
            result.files
        );
    }

    #[rstest]
    #[case::within_limit(
        &["c.txt", "a.txt", "b.txt"],
        1000,
        vec!["a.txt", "b.txt", "c.txt"],
        false,
    )]
    #[case::truncated(
        &["e.txt", "c.txt", "a.txt", "d.txt", "b.txt"],
        3,
        vec!["a.txt", "b.txt", "c.txt"],
        true,
    )]
    fn glob_sorts_deterministically_with_identical_mtimes(
        #[case] files: &[&str],
        #[case] limit: usize,
        #[case] expected: Vec<&str>,
        #[case] expected_truncated: bool,
    ) {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        let same_time = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        for name in files {
            let f = File::create(base.join(name)).unwrap();
            f.set_times(FileTimes::new().set_modified(same_time))
                .unwrap();
        }

        let result = run_glob_with_limit(base, "**/*.txt", limit);

        assert_eq!(
            result.files, expected,
            "entries with identical mtimes must be sorted lexicographically by path"
        );
        assert_eq!(result.truncated, expected_truncated);
    }

    #[test]
    fn glob_returns_forward_slash_paths() {
        let dir = create_test_tree();
        let result = run_glob(dir.path(), "**/*.rs");

        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib.rs"));

        for path in &result.files {
            assert!(!path.contains('\\'), "expected forward slashes: {path}");
        }
        assert!(result.files.iter().any(|f| f.contains('/')));
    }

    /// Verifies that glob_files filters results via `is_path_allowed` using both
    /// relative and absolute search paths.
    #[rstest]
    #[case::relative_path(".")]
    #[case::absolute_path_uses_workdir_as_param(
        // Placeholder: replaced with the temp dir path inside the test body.
        "ABSOLUTE"
    )]
    fn glob_filters_via_is_path_allowed(#[case] path_kind: &str) -> TestResult {
        let dir = TempDir::new()?;
        fs::create_dir_all(dir.path().join("src"))?;
        File::create(dir.path().join("src/lib.rs"))?;
        File::create(dir.path().join("Cargo.toml"))?;

        let root = soft_canonicalize(dir.path())?;
        let policy = GlobPolicy::builder_with_base(&root)?
            .allow("src/**")?
            .build()?;
        let resolver = AllowedGlobResolver::new(dir.path())?.with_policy(policy);

        let search_path = if path_kind == "ABSOLUTE" {
            dir.path().to_str().unwrap().to_string()
        } else {
            path_kind.to_string()
        };

        let result = glob_files(
            &resolver,
            GlobRequest {
                pattern: "**/*".to_string(),
                path: search_path,
            },
            &GlobSettings::new(),
        )?;

        assert!(result.files.iter().any(|f| f.contains("lib.rs")));
        assert!(!result.files.iter().any(|f| f.contains("Cargo.toml")));
        Ok(())
    }

    /// Creates a nested directory tree with root and nested `.gitignore` files
    /// to verify multi-level ignore rule application.
    fn create_nested_gitignore_tree() -> TempDir {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        fs::create_dir_all(base.join(".git")).unwrap();

        let mut root_gi = File::create(base.join(".gitignore")).unwrap();
        writeln!(root_gi, "build/").unwrap();

        fs::create_dir_all(base.join("src/subdir")).unwrap();
        File::create(base.join("src/main.rs")).unwrap();
        File::create(base.join("src/subdir/keep.rs")).unwrap();

        fs::create_dir_all(base.join("build")).unwrap();
        File::create(base.join("build/output.rs")).unwrap();

        let mut subdir_gi = File::create(base.join("src/subdir/.gitignore")).unwrap();
        writeln!(subdir_gi, "*.tmp").unwrap();
        File::create(base.join("src/subdir/test.tmp")).unwrap();

        dir
    }

    /// Verifies that the root `.gitignore` (`build/`) prevents `build/output.rs`
    /// from appearing in results even when the glob pattern (`**/*.rs`) would
    /// otherwise match it, while non-ignored `.rs` files remain visible.
    #[test]
    fn glob_root_gitignore_excludes_build_dir_with_rs_pattern() {
        let dir = create_nested_gitignore_tree();
        let result = run_glob(dir.path(), "**/*.rs");

        assert!(
            !result.files.iter().any(|f| f.contains("build")),
            "root .gitignore excludes build/, but build/output.rs appeared: {:?}",
            result.files
        );
        assert!(
            result.files.iter().any(|f| f.contains("main.rs")),
            "expected src/main.rs in results: {:?}",
            result.files
        );
    }

    /// Verifies that a nested `.gitignore` (`src/subdir/.gitignore` with `*.tmp`)
    /// excludes matching files in that subtree, while sibling files not matching
    /// the pattern (`keep.rs`) remain in the results.
    #[test]
    fn glob_nested_gitignore_excludes_tmp_files() {
        let dir = create_nested_gitignore_tree();
        let result = run_glob(dir.path(), "**/*");

        assert!(
            !result.files.iter().any(|f| f.contains("test.tmp")),
            "nested .gitignore excludes *.tmp, but src/subdir/test.tmp appeared: {:?}",
            result.files
        );
        assert!(
            result.files.iter().any(|f| f.contains("keep.rs")),
            "expected src/subdir/keep.rs in results: {:?}",
            result.files
        );
    }

    #[test]
    fn glob_handles_empty_directory() {
        let dir = TempDir::new().unwrap();
        let result = run_glob(dir.path(), "**/*");

        assert!(
            result.files.is_empty(),
            "empty dir should yield no files: {:?}",
            result.files
        );
        assert!(!result.truncated);
        assert!(!result.partial);
    }

    #[test]
    fn glob_handles_pattern_matching_nothing() {
        let dir = create_test_tree();
        let result = run_glob(dir.path(), "**/*.xyz");

        assert!(
            result.files.is_empty(),
            "no .xyz files exist: {:?}",
            result.files
        );
        assert!(!result.truncated);
    }

    #[test]
    #[cfg(unix)]
    fn glob_does_not_follow_symlinks_to_directories() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().unwrap();
        let base = dir.path();

        fs::create_dir_all(base.join("real_dir")).unwrap();
        File::create(base.join("real_dir/inside.txt")).unwrap();
        File::create(base.join("top.txt")).unwrap();

        symlink(base.join("real_dir"), base.join("link_dir")).unwrap();

        let result = run_glob(base, "**/*.txt");

        assert!(
            !result.files.iter().any(|f| f.starts_with("link_dir/")),
            "symlinked directory contents should not be traversed via link_dir: {:?}",
            result.files
        );
        assert!(
            result.files.iter().any(|f| f == "real_dir/inside.txt"),
            "real directory contents should still be traversed: {:?}",
            result.files
        );
        assert!(
            result.files.iter().any(|f| f == "top.txt"),
            "expected top.txt in results: {:?}",
            result.files
        );
    }

    #[test]
    fn glob_sets_truncated_flag_when_results_exceed_limit() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();
        for i in 0..5 {
            File::create(base.join(format!("file{i}.txt"))).unwrap();
        }

        let result = run_glob_with_limit(base, "**/*.txt", 3);

        assert_eq!(result.files.len(), 3, "should return exactly limit items");
        assert!(result.truncated, "truncated flag should be set");
    }

    #[test]
    fn glob_truncated_false_when_within_limit() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();
        File::create(base.join("a.txt")).unwrap();
        File::create(base.join("b.txt")).unwrap();

        let result = run_glob_with_limit(base, "**/*.txt", 100);

        assert!(
            !result.truncated,
            "truncated should be false when files <= limit"
        );
    }

    #[test]
    fn glob_nested_gitignore_without_root_gitignore() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        fs::create_dir_all(base.join(".git")).unwrap();
        fs::create_dir_all(base.join("src/subdir")).unwrap();
        File::create(base.join("src/main.rs")).unwrap();
        File::create(base.join("src/subdir/keep.rs")).unwrap();

        let mut subdir_gi = File::create(base.join("src/subdir/.gitignore")).unwrap();
        writeln!(subdir_gi, "*.tmp").unwrap();
        File::create(base.join("src/subdir/test.tmp")).unwrap();

        let result = run_glob(base, "**/*");

        assert!(
            !result.files.iter().any(|f| f.contains("test.tmp")),
            "nested .gitignore should exclude *.tmp even without root .gitignore: {:?}",
            result.files
        );
    }
}
