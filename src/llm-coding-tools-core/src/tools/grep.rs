//! Grep content search operation.

use crate::error::{ToolError, ToolResult};
use crate::path::PathResolver;
use crate::tool_metadata::grep as grep_meta;
use crate::util::{push_usize, truncate_line_with_ellipsis, TRUNCATION_ELLIPSIS};
use globset::Glob;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::SystemTime;

/// Default maximum line length (in characters) for formatted grep output.
pub const DEFAULT_MAX_LINE_LENGTH: usize = 2000;

/// Estimated characters per grep match for buffer pre-allocation.
const ESTIMATED_CHARS_PER_MATCH: usize = 128;

/// Serde-friendly grep request owned by the core crate.
#[derive(Debug, Deserialize)]
pub struct GrepRequest {
    pub pattern: String,
    pub path: String,
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl GrepRequest {
    /// Parses a raw JSON tool payload into a grep request.
    ///
    /// # Errors
    /// - Returns [`ToolError::Json`] when the JSON payload cannot be deserialized
    ///   into a [`GrepRequest`] (e.g., missing required `pattern` or `path` fields,
    ///   or invalid field types).
    pub fn parse(args: Value) -> ToolResult<Self> {
        serde_json::from_value(args).map_err(ToolError::from)
    }
}

/// Runtime settings applied to grep requests.
///
/// The `max_limit` field caps the number of matching lines returned, even if
/// the caller requests more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepSettings {
    max_limit: usize,
}

impl Default for GrepSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl GrepSettings {
    /// Creates valid grep search settings with the standard defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_limit: grep_meta::DEFAULT_LIMIT,
        }
    }

    /// Sets the upper bound on matching lines returned per search.
    ///
    /// # Errors
    /// - Returns an error when `max_limit` is below [`MIN_LIMIT`].
    ///
    /// [`MIN_LIMIT`]: crate::util::MIN_LIMIT
    pub fn with_max_limit(mut self, max_limit: usize) -> ToolResult<Self> {
        use crate::util::MIN_LIMIT;
        if max_limit < MIN_LIMIT {
            return Err(ToolError::validation_for(
                "max_limit",
                format!("max_limit must be >= {}", MIN_LIMIT),
            ));
        }
        self.max_limit = max_limit;
        Ok(self)
    }

    /// Returns the upper bound on matching lines returned per search.
    ///
    /// # Returns
    /// - The configured maximum line limit.
    #[must_use]
    pub const fn max_limit(&self) -> usize {
        self.max_limit
    }
}

/// Formatting settings for rendered grep output.
///
/// Controls how matching lines are displayed: truncation length for long lines
/// and whether to prefix each line with a line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrepFormattingSettings {
    max_line_length: usize,
    line_numbers: bool,
}

impl Default for GrepFormattingSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl GrepFormattingSettings {
    /// Creates valid grep formatting settings with the standard line-numbered output.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_line_length: DEFAULT_MAX_LINE_LENGTH,
            line_numbers: true,
        }
    }

    /// Updates the maximum line length for formatted output.
    ///
    /// # Errors
    /// - Returns an error when `max_line_length` is below
    ///   [`MIN_LINE_LENGTH`].
    ///
    /// [`MIN_LINE_LENGTH`]: crate::util::MIN_LINE_LENGTH
    pub fn with_max_line_length(mut self, max_line_length: usize) -> ToolResult<Self> {
        use crate::util::MIN_LINE_LENGTH;
        if max_line_length < MIN_LINE_LENGTH {
            return Err(ToolError::validation_for(
                "max_line_length",
                format!("max_line_length must be >= {}", MIN_LINE_LENGTH),
            ));
        }
        self.max_line_length = max_line_length;
        Ok(self)
    }

    /// Enables or disables line numbers in output.
    #[must_use]
    pub const fn with_line_numbers(mut self, line_numbers: bool) -> Self {
        self.line_numbers = line_numbers;
        self
    }

    /// Returns the maximum line length for formatted output.
    #[must_use]
    pub const fn max_line_length(self) -> usize {
        self.max_line_length
    }

    /// Returns whether line numbers are included in output.
    #[must_use]
    pub const fn line_numbers(self) -> bool {
        self.line_numbers
    }
}

/// A single line match within a file.
#[derive(Debug, Clone, Serialize)]
pub struct GrepLineMatch {
    /// 1-indexed line number.
    pub line_num: u64,
    /// Content of the matched line.
    pub line_text: String,
}

/// All matches within a single file.
#[derive(Debug, Clone, Serialize)]
pub struct GrepFileMatches {
    /// File path.
    pub path: String,
    /// Matches in this file, in line order.
    pub matches: Vec<GrepLineMatch>,
    #[serde(skip)]
    pub(crate) mtime: SystemTime,
}

/// Output from grep search.
#[derive(Debug, Serialize)]
pub struct GrepOutput {
    /// Files with matches, sorted by modification time (newest first).
    pub files: Vec<GrepFileMatches>,
    /// Total match count across all files.
    pub match_count: usize,
    /// Whether results were truncated due to limit.
    pub truncated: bool,
    /// Whether one or more files could not be searched.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// Per-file traversal/search errors encountered while collecting matches.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Effective match limit applied to the search.
    #[serde(skip)]
    pub effective_limit: usize,
}

impl GrepOutput {
    /// Formats grep results as human-readable text.
    ///
    /// # Arguments
    ///
    /// * `formatting` - The formatting settings to use for output
    pub fn format(&self, formatting: GrepFormattingSettings) -> String {
        let line_numbers = formatting.line_numbers();
        let max_line_len = formatting.max_line_length();
        let estimated_capacity = self.match_count * ESTIMATED_CHARS_PER_MATCH;
        let mut output = String::with_capacity(estimated_capacity);

        output.push_str("Found ");
        push_usize(&mut output, self.match_count);
        output.push_str(" matches\n");

        for file in &self.files {
            output.push('\n');
            output.push_str(&file.path);
            output.push_str(":\n");

            for m in &file.matches {
                let (display_text, was_truncated) =
                    truncate_line_with_ellipsis(&m.line_text, max_line_len);

                if line_numbers {
                    output.push_str("  L");
                    push_usize(&mut output, m.line_num as usize);
                    output.push_str(": ");
                    output.push_str(display_text);
                } else {
                    output.push_str("  ");
                    output.push_str(display_text);
                }

                if was_truncated {
                    output.push_str(TRUNCATION_ELLIPSIS);
                }

                output.push('\n');
            }
        }

        if self.truncated {
            output.push_str("\n(Results truncated at ");
            push_usize(&mut output, self.effective_limit);
            output.push_str(" matches)");
        }

        if self.partial {
            output.push_str("\n(Partial results: ");
            push_usize(&mut output, self.errors.len());
            output.push_str(" file error(s) encountered)");
        }

        output
    }
}

/// Searches for content matching a regex pattern.
///
/// Results are sorted by modification time (newest first).
/// Binary files are automatically skipped.
pub fn grep_search<R: PathResolver>(
    resolver: &R,
    request: GrepRequest,
    settings: &GrepSettings,
) -> ToolResult<GrepOutput> {
    let pattern = request.pattern.trim();
    if pattern.is_empty() {
        return Err(ToolError::validation_for(
            "pattern",
            "pattern must not be empty or whitespace-only",
        ));
    }

    let limit = request
        .limit
        .unwrap_or(settings.max_limit())
        .min(settings.max_limit());
    if limit == 0 {
        return Err(ToolError::validation_for("limit", "limit must be >= 1"));
    }

    let include = request.include.as_deref().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let path = resolver.resolve(&request.path)?;

    let matcher =
        RegexMatcher::new(pattern).map_err(|e| ToolError::InvalidPattern(e.to_string()))?;

    // Optional filename filter via glob.
    let glob_matcher = include
        .map(|pattern| Glob::new(pattern).map(|glob| glob.compile_matcher()))
        .transpose()?;

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let mut files: Vec<GrepFileMatches> = Vec::with_capacity(64);
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

        // Skip directories and non-regular files.
        match entry.file_type() {
            Some(ft) if ft.is_file() => {}
            _ => continue,
        }

        let entry_path = entry.path();

        // Filter entries through the resolver's path policy.
        if !resolver.is_path_allowed(entry_path) {
            continue;
        }

        // Apply include glob to basename when requested.
        if let Some(ref matcher) = glob_matcher {
            let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };
            if !matcher.is_match(file_name) {
                continue;
            }
        }

        let search_result = collect_file_matches(&matcher, &mut searcher, entry_path);

        if let Some(error) = search_result.error {
            errors.push(error);
        }

        if search_result.matches.is_empty() {
            continue;
        }

        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        files.push(GrepFileMatches {
            path: entry_path.to_string_lossy().into_owned(),
            matches: search_result.matches,
            mtime,
        });
    }

    // Sort newest files first.
    files.sort_by_key(|file| std::cmp::Reverse(file.mtime));

    let mut match_count = 0;
    let mut truncate_at = files.len();
    let mut truncated = false;

    // Enforce overall match limit across files.
    for (x, file) in files.iter_mut().enumerate() {
        let remaining = limit - match_count;
        if file.matches.len() > remaining {
            file.matches.truncate(remaining);
            match_count += remaining;
            truncate_at = x + 1;
            truncated = true;
            break;
        }
        match_count += file.matches.len();
    }

    files.truncate(truncate_at);

    Ok(GrepOutput {
        files,
        match_count,
        truncated,
        partial: !errors.is_empty(),
        errors,
        effective_limit: limit,
    })
}

struct FileSearchResult {
    matches: Vec<GrepLineMatch>,
    error: Option<String>,
}

#[inline]
fn collect_file_matches(
    matcher: &RegexMatcher,
    searcher: &mut Searcher,
    path: &Path,
) -> FileSearchResult {
    let mut matches = Vec::new();

    let error = searcher
        .search_path(
            matcher,
            path,
            UTF8(|line_num, line| {
                matches.push(GrepLineMatch {
                    line_num,
                    line_text: line.trim_end().to_string(),
                });
                Ok(true)
            }),
        )
        .err()
        .map(|err| format!("failed to search '{}': {err}", path.display()));

    FileSearchResult { matches, error }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::AbsolutePathResolver;
    use crate::path::AllowedGlobResolver;
    use crate::path::GlobPolicy;
    use rstest::rstest;
    use soft_canonicalize::soft_canonicalize;
    use std::fs;
    use tempfile::tempdir;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    // GrepSettings and GrepFormattingSettings tests
    #[test]
    fn grep_settings_should_create_standard_defaults() {
        let settings = GrepSettings::new();
        assert_eq!(settings.max_limit(), grep_meta::DEFAULT_LIMIT);
    }

    #[test]
    fn grep_settings_should_reject_zero_limit() {
        assert!(GrepSettings::new().with_max_limit(0).is_err());
    }

    #[test]
    fn grep_formatting_settings_should_create_standard_defaults() {
        let settings = GrepFormattingSettings::new();
        assert_eq!(settings.max_line_length(), DEFAULT_MAX_LINE_LENGTH);
        assert!(settings.line_numbers());
    }

    #[test]
    fn grep_formatting_settings_should_reject_short_line_length() {
        assert!(GrepFormattingSettings::new()
            .with_max_line_length(3)
            .is_err());
    }

    /// Verifies that grep search returns the expected number of files and matches
    /// for different file layouts and optional glob filters.
    #[rstest]
    #[case::single_file_no_filter(
        vec![("match.txt", "hello world")], // files: 1 file with 1 match
        "hello",                            // pattern: matches 1 place
        None::<&str>,                       // filter: none (search all)
        1,                                  // expected: 1 file matched
        1                                   // expected: 1 total match
    )]
    #[case::glob_filters_to_rs_only(
        vec![("match.rs", "hello"), ("match.txt", "hello")], // files: 2 files, same pattern
        "hello",                                             // pattern: matches 1 place in each
        Some("*.rs"),                                        // filter: only .rs files
        1,                                                   // expected: 1 file matched (.rs only)
        1                                                    // expected: 1 total match
    )]
    #[case::no_matches_found(
        vec![("notes.txt", "goodbye")], // files: 1 file with no match
        "hello",                        // pattern: matches 0 places
        None::<&str>,                   // filter: none (search all)
        0,                              // expected: 0 files matched
        0                               // expected: 0 total matches
    )]
    #[case::multiple_files_multiple_matches(
        vec![("a.txt", "hello\nworld"), ("b.txt", "hello\nhello")], // files: 2 files
        "hello",                                                    // pattern: matches 1+2 places
        None::<&str>,                                               // filter: none (search all)
        2,                                                          // expected: 2 files matched
        3                                                           // expected: 3 total matches
    )]
    #[case::glob_excludes_everything(
        vec![("match.rs", "hello")], // files: 1 file that would match
        "hello",                     // pattern: matches 0 places (file excluded)
        Some("*.py"),                // filter: only .py files (excludes .rs)
        0,                           // expected: 0 files matched (all excluded)
        0                            // expected: 0 total matches
    )]
    fn grep_search_finds_expected_matches(
        #[case] files: Vec<(&str, &str)>,
        #[case] pattern: &str,
        #[case] include: Option<&str>,
        #[case] expected_file_count: usize,
        #[case] expected_match_count: usize,
    ) {
        let temp = tempdir().unwrap();
        for (name, content) in files {
            std::fs::write(temp.path().join(name), content).unwrap();
        }
        let resolver = AbsolutePathResolver;

        let result = grep_search(
            &resolver,
            GrepRequest {
                pattern: pattern.to_string(),
                path: temp.path().to_str().unwrap().to_string(),
                include: include.map(|s| s.to_string()),
                limit: None,
            },
            &GrepSettings::new().with_max_limit(10).unwrap(),
        )
        .unwrap();

        assert_eq!(result.files.len(), expected_file_count);
        assert_eq!(result.match_count, expected_match_count);

        // Verify glob filtering works correctly
        if let Some(glob) = include {
            for file in &result.files {
                assert!(file
                    .path
                    .ends_with(glob.trim_start_matches('*').trim_start_matches('.')));
            }
        }
    }

    /// Verifies that format output displays correct status markers for different
    /// combinations of partial results and truncation flags.
    #[rstest]
    #[case::partial_only(true, false, true, false)]
    #[case::truncated_only(false, true, false, true)]
    #[case::both_flags(true, true, true, true)]
    #[case::neither_flag(false, false, false, false)]
    fn grep_format_displays_status_markers(
        #[case] partial: bool,
        #[case] truncated: bool,
        #[case] expect_partial_msg: bool,
        #[case] expect_truncated_msg: bool,
    ) {
        let errors = if partial {
            vec!["walk error: denied".to_string()]
        } else {
            Vec::new()
        };

        let output = GrepOutput {
            files: Vec::new(),
            match_count: 0,
            truncated,
            partial,
            errors,
            effective_limit: 10,
        };

        let formatted = output.format(GrepFormattingSettings::new());

        assert_eq!(formatted.contains("Partial results"), expect_partial_msg);
        assert_eq!(
            formatted.contains("Results truncated"),
            expect_truncated_msg
        );
    }

    /// Verifies how collect_file_matches handles various file edge cases like
    /// missing files and binary content.
    #[rstest]
    #[case::missing_file(
        "missing.txt", // file: does not exist on disk
        true,          // expect_error: search fails (file not found)
        0              // expected_match_count: 0 matches (search never ran)
    )]
    #[case::binary_file(
        "binary.bin",  // file: binary with null bytes
        false,         // expect_error: no error (binary skipped gracefully)
        0              // expected_match_count: 0 matches (binary not searched)
    )]
    fn collect_file_matches_handles_edge_cases(
        #[case] file_name: &str,
        #[case] expect_error: bool,
        #[case] expected_match_count: usize,
    ) {
        let temp = tempdir().unwrap();
        let target_path = temp.path().join(file_name);

        // Create binary file if testing binary detection
        if file_name == "binary.bin" {
            std::fs::write(&target_path, b"hello\x00world").unwrap();
        }

        let matcher = RegexMatcher::new("hello").unwrap();
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(0))
            .build();

        let result = collect_file_matches(&matcher, &mut searcher, &target_path);

        assert_eq!(result.matches.len(), expected_match_count);
        assert_eq!(result.error.is_some(), expect_error);
    }

    /// Verifies that line truncation in formatted output behaves correctly for
    /// different line lengths and line number settings.
    #[rstest]
    #[case::with_line_numbers_short(
        6,           // max_len: line "abcdefghij" (10 chars) truncated to 6
        true,        // with_line_numbers: yes, shows "L1: " prefix
        "L1: abc..." // expected: truncated with line number prefix
    )]
    #[case::without_line_numbers_short(
        4,        // max_len: line truncated to 4 chars
        false,    // with_line_numbers: no prefix
        "  a..."  // expected: truncated without line number prefix
    )]
    #[case::no_truncation_when_fits(
        200,             // max_len: larger than line length (10 chars)
        true,            // with_line_numbers: yes
        "L1: abcdefghij" // expected: full line preserved, no truncation
    )]
    #[case::exact_boundary_no_truncation(
        10,              // max_len: exactly matches line length (10 chars)
        true,            // with_line_numbers: yes
        "L1: abcdefghij" // expected: full line preserved, boundary not exceeded
    )]
    fn grep_format_handles_line_truncation(
        #[case] max_len: usize,
        #[case] with_line_numbers: bool,
        #[case] expected: &str,
    ) {
        let output = GrepOutput {
            files: vec![GrepFileMatches {
                path: "file.txt".to_string(),
                matches: vec![GrepLineMatch {
                    line_num: 1,
                    line_text: "abcdefghij".to_string(),
                }],
                mtime: SystemTime::UNIX_EPOCH,
            }],
            match_count: 1,
            truncated: false,
            partial: false,
            errors: Vec::new(),
            effective_limit: 10,
        };

        let formatted = output.format(
            GrepFormattingSettings::new()
                .with_max_line_length(max_len)
                .unwrap()
                .with_line_numbers(with_line_numbers),
        );

        assert!(
            formatted.contains(expected),
            "Expected '{}' in:\n{}",
            expected,
            formatted
        );
    }

    #[test]
    fn grep_marks_results_partial_when_walker_reports_error() {
        let temp = tempdir().unwrap();
        let missing_root = temp.path().join("missing-root");
        let resolver = AbsolutePathResolver;

        let result = grep_search(
            &resolver,
            GrepRequest {
                pattern: "hello".to_string(),
                path: missing_root.to_str().unwrap().to_string(),
                include: None,
                limit: None,
            },
            &GrepSettings::new().with_max_limit(10).unwrap(),
        )
        .unwrap();

        // Walker errors should mark results as partial but not truncated
        assert!(result.partial);
        assert!(!result.truncated);
        assert!(!result.errors.is_empty());
        assert_eq!(result.match_count, 0);
    }

    #[test]
    fn grep_request_rejects_empty_pattern() {
        let temp = tempdir().unwrap();
        let resolver = AbsolutePathResolver;

        let err = grep_search(
            &resolver,
            GrepRequest {
                pattern: "   ".into(),
                path: temp.path().to_string_lossy().into_owned(),
                include: None,
                limit: None,
            },
            &GrepSettings::new().with_max_limit(10).unwrap(),
        )
        .unwrap_err();

        assert!(matches!(err, ToolError::Validation { .. }));
        assert_eq!(
            err.to_string(),
            "validation error: pattern must not be empty or whitespace-only"
        );
    }

    #[test]
    fn grep_request_ignores_blank_include_and_respects_limit() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "hello\nworld\n").unwrap();
        std::fs::write(temp.path().join("b.txt"), "hello\nhello\n").unwrap();
        let resolver = AbsolutePathResolver;

        let result = grep_search(
            &resolver,
            GrepRequest {
                pattern: " hello ".into(),
                path: temp.path().to_string_lossy().into_owned(),
                include: Some("   ".into()),
                limit: Some(1),
            },
            &GrepSettings::new().with_max_limit(1).unwrap(),
        )
        .unwrap();

        assert_eq!(result.match_count, 1);
        assert!(result.truncated);
        assert_eq!(result.effective_limit, 1);
    }

    #[test]
    fn grep_request_caps_limit_when_request_limit_exceeds_max() {
        let temp = tempdir().unwrap();
        // Create file with 5 matching lines
        std::fs::write(
            temp.path().join("test.txt"),
            "hello\nhello\nhello\nhello\nhello\n",
        )
        .unwrap();
        let resolver = AbsolutePathResolver;

        let result = grep_search(
            &resolver,
            GrepRequest {
                pattern: "hello".into(),
                path: temp.path().to_string_lossy().into_owned(),
                include: None,
                limit: Some(100), // Request asks for 100 matches
            },
            &GrepSettings::new().with_max_limit(2).unwrap(), // But max_limit is only 2
        )
        .unwrap();

        // Verify that the limit is capped to max_limit
        assert_eq!(result.effective_limit, 2);
        assert_eq!(result.match_count, 2);
        assert!(result.truncated);
    }

    /// Verifies that grep_search filters results via `is_path_allowed` using both
    /// relative and absolute search paths.
    #[rstest]
    #[case::relative_path(".")]
    #[case::absolute_path_uses_workdir_as_param(
        // Placeholder: replaced with the temp dir path inside the test body.
        "ABSOLUTE"
    )]
    fn grep_filters_via_is_path_allowed(#[case] path_kind: &str) -> TestResult {
        let temp = tempdir()?;
        fs::create_dir_all(temp.path().join("src"))?;
        std::fs::write(temp.path().join("src/lib.rs"), "match_content\n")?;
        std::fs::write(temp.path().join("Cargo.toml"), "match_content\n")?;

        let root = soft_canonicalize(temp.path())?;
        let policy = GlobPolicy::builder_with_base(&root)?
            .allow("src/**")?
            .build()?;
        let resolver = AllowedGlobResolver::new(temp.path())?.with_policy(policy);

        let search_path = if path_kind == "ABSOLUTE" {
            temp.path().to_str().unwrap().to_string()
        } else {
            path_kind.to_string()
        };

        let result = grep_search(
            &resolver,
            GrepRequest {
                pattern: "match_content".into(),
                path: search_path,
                include: None,
                limit: None,
            },
            &GrepSettings::new().with_max_limit(100)?,
        )?;

        assert!(
            result.files.iter().any(|f| f.path.contains("lib.rs")),
            "src/lib.rs should appear in results"
        );
        assert!(
            !result.files.iter().any(|f| f.path.contains("Cargo.toml")),
            "Cargo.toml should be filtered by src/** policy"
        );
        Ok(())
    }
}
