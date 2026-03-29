//! Grep content search operation.

use crate::error::{ToolError, ToolResult};
use crate::path::PathResolver;
use crate::util::{truncate_line_with_ellipsis, TRUNCATION_ELLIPSIS};
use globset::Glob;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::WalkBuilder;
use serde::Serialize;
use std::fmt::Write;
use std::path::Path;
use std::time::SystemTime;

/// Default maximum line length (in characters) for formatted grep output.
pub const DEFAULT_MAX_LINE_LENGTH: usize = 2000;

/// Estimated characters per grep match for buffer pre-allocation.
const ESTIMATED_CHARS_PER_MATCH: usize = 128;

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
}

impl GrepOutput {
    /// Formats grep results as human-readable text.
    ///
    /// # Type Parameters
    ///
    /// * `LINE_NUMBERS` - When `true`, prefixes each match with `L{num}: `
    ///
    /// # Arguments
    ///
    /// * `limit` - The original match limit (used in truncation message)
    /// * `max_line_len` - Truncate lines exceeding this character length and append `...`
    pub fn format<const LINE_NUMBERS: bool>(&self, limit: usize, max_line_len: usize) -> String {
        let estimated_capacity = self.match_count * ESTIMATED_CHARS_PER_MATCH;
        let mut output = String::with_capacity(estimated_capacity);

        let _ = writeln!(&mut output, "Found {} matches", self.match_count);

        for file in &self.files {
            let _ = writeln!(&mut output, "\n{}:", file.path);
            for m in &file.matches {
                let (display_text, was_truncated) =
                    truncate_line_with_ellipsis(&m.line_text, max_line_len);

                if LINE_NUMBERS {
                    let _ = write!(&mut output, "  L{}: {}", m.line_num, display_text);
                } else {
                    let _ = write!(&mut output, "  {}", display_text);
                }

                if was_truncated {
                    output.push_str(TRUNCATION_ELLIPSIS);
                }

                output.push('\n');
            }
        }

        if self.truncated {
            let _ = write!(&mut output, "\n(Results truncated at {} matches)", limit);
        }

        if self.partial {
            let _ = write!(
                &mut output,
                "\n(Partial results: {} file error(s) encountered)",
                self.errors.len()
            );
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
    pattern: &str,
    include: Option<&str>,
    search_path: &str,
    limit: usize,
) -> ToolResult<GrepOutput> {
    let path = resolver.resolve(search_path)?;

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
    use rstest::rstest;
    use tempfile::tempdir;

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
            pattern,
            include,
            temp.path().to_str().unwrap(),
            10,
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
        };

        let formatted = output.format::<true>(10, DEFAULT_MAX_LINE_LENGTH);

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
        };

        let formatted = if with_line_numbers {
            output.format::<true>(10, max_len)
        } else {
            output.format::<false>(10, max_len)
        };

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

        let result =
            grep_search(&resolver, "hello", None, missing_root.to_str().unwrap(), 10).unwrap();

        // Walker errors should mark results as partial but not truncated
        assert!(result.partial);
        assert!(!result.truncated);
        assert!(!result.errors.is_empty());
        assert_eq!(result.match_count, 0);
    }
}
