//! Glob pattern file matching operation.

use crate::error::{ToolError, ToolResult};
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
    pub pattern: String,
    pub path: String,
}

impl GlobRequest {
    /// Parses a raw JSON tool payload into a glob request.
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to glob requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobSettings {
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
    pub const fn new() -> Self {
        Self {
            limit: glob_meta::MAX_RESULTS,
        }
    }

    /// Updates the maximum number of files returned.
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
    #[must_use]
    pub const fn limit(self) -> usize {
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
pub fn glob_files<R: PathResolver>(
    resolver: &R,
    request: GlobRequest,
    settings: GlobSettings,
) -> ToolResult<GlobOutput> {
    let path = resolver.resolve(&request.path)?;

    if !path.is_dir() {
        return Err(ToolError::InvalidPath(format!(
            "path is not a directory: {}",
            path.display()
        )));
    }

    let limit = settings.limit();

    let matcher = Glob::new(&request.pattern)?.compile_matcher();

    let mut files_with_mtime: Vec<(String, SystemTime)> = Vec::new();
    let mut errors: Vec<String> = Vec::with_capacity(8);

    let walker = WalkBuilder::new(&path)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                errors.push(format!("walk error: {err}"));
                continue;
            }
        };

        if let Some(ft) = entry.file_type() {
            if ft.is_dir() {
                continue;
            }
        } else {
            continue;
        }

        let rel_path = match entry.path().strip_prefix(&path) {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(err) => {
                errors.push(format!(
                    "failed to make '{}' relative to '{}': {err}",
                    entry.path().display(),
                    path.display()
                ));
                continue;
            }
        };

        // Normalise Windows backslashes to forward slashes for glob pattern matching
        #[cfg(windows)]
        let rel_path = rel_path.replace('\\', "/");

        if rel_path.is_empty() {
            continue;
        }

        if !matcher.is_match(&rel_path) {
            continue;
        }

        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        files_with_mtime.push((rel_path, mtime));
    }

    files_with_mtime.sort_by_key(|entry| std::cmp::Reverse(entry.1));

    let truncated = files_with_mtime.len() > limit;

    let files: Vec<String> = files_with_mtime
        .into_iter()
        .take(limit)
        .map(|(path, _)| path)
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
    use super::*;
    use crate::path::AbsolutePathResolver;
    use rstest::rstest;
    use std::fs::{self, File, FileTimes};
    use std::io::Write;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

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

    // GlobSettings tests
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
        let resolver = AbsolutePathResolver;

        let result = glob_files(
            &resolver,
            GlobRequest {
                pattern: pattern.to_string(),
                path: dir.path().to_str().unwrap().to_string(),
            },
            GlobSettings::new().with_limit(1000).unwrap(),
        )
        .unwrap();

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
        let resolver = AbsolutePathResolver;

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

        let result = glob_files(
            &resolver,
            GlobRequest {
                pattern: "**/*.txt".to_string(),
                path: base.to_str().unwrap().to_string(),
            },
            GlobSettings::new().with_limit(1000).unwrap(),
        )
        .unwrap();

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

        // Newer files should appear before older ones in the results
        assert!(
            newer_index < older_index,
            "expected newer file before older: {:?}",
            result.files
        );
    }

    #[test]
    fn glob_returns_forward_slash_paths() {
        // Patterns and returned paths use forward slashes on all platforms
        let dir = create_test_tree();
        let resolver = AbsolutePathResolver;
        let result = glob_files(
            &resolver,
            GlobRequest {
                pattern: "**/*.rs".to_string(),
                path: dir.path().to_str().unwrap().to_string(),
            },
            GlobSettings::new().with_limit(1000).unwrap(),
        )
        .unwrap();

        // The only .rs file in our test tree is src/lib.rs
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib.rs"));

        // Returned paths must always use forward slashes (important on Windows)
        for path in &result.files {
            assert!(!path.contains('\\'), "expected forward slashes: {path}");
        }
        assert!(result.files.iter().any(|f| f.contains('/')));
    }
}
