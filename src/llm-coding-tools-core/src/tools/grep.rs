//! Grep content search operation.

use crate::error::{ToolError, ToolResult};
use crate::path::PathResolver;
use globset::Glob;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::WalkBuilder;
use serde::Serialize;
use std::fmt::Write;
use std::path::Path;
use std::time::SystemTime;

/// Default maximum line length (in bytes) for formatted grep output.
pub const DEFAULT_MAX_LINE_LENGTH: usize = 2000;

/// Above average length of a file path.
const ESTIMATED_CHARS_PER_LINE: usize = 128;

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
    /// * `max_line_len` - Truncate lines exceeding this byte length at UTF-8 boundary
    pub fn format<const LINE_NUMBERS: bool>(&self, limit: usize, max_line_len: usize) -> String {
        let estimated_capacity = self.match_count * ESTIMATED_CHARS_PER_LINE;
        let mut output = String::with_capacity(estimated_capacity);

        let _ = writeln!(&mut output, "Found {} matches", self.match_count);

        for file in &self.files {
            let _ = writeln!(&mut output, "\n{}:", file.path);
            for m in &file.matches {
                let truncated_text = if m.line_text.len() > max_line_len {
                    &m.line_text[..m.line_text.floor_char_boundary(max_line_len)]
                } else {
                    &m.line_text
                };
                if LINE_NUMBERS {
                    let _ = writeln!(&mut output, "  L{}: {}", m.line_num, truncated_text);
                } else {
                    let _ = writeln!(&mut output, "  {}", truncated_text);
                }
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
    use tempfile::tempdir;

    #[test]
    fn grep_finds_matches() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("match.txt"), "hello world").unwrap();
        let resolver = AbsolutePathResolver;

        let result =
            grep_search(&resolver, "hello", None, temp.path().to_str().unwrap(), 10).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.match_count, 1);
    }

    #[test]
    fn grep_respects_glob_filter() {
        let temp = tempdir().unwrap();
        std::fs::write(temp.path().join("match.rs"), "hello").unwrap();
        std::fs::write(temp.path().join("match.txt"), "hello").unwrap();
        let resolver = AbsolutePathResolver;

        let result = grep_search(
            &resolver,
            "hello",
            Some("*.rs"),
            temp.path().to_str().unwrap(),
            10,
        )
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].path.ends_with(".rs"));
    }

    #[test]
    fn grep_format_includes_partial_marker() {
        let output = GrepOutput {
            files: Vec::new(),
            match_count: 0,
            truncated: false,
            partial: true,
            errors: vec!["walk error: denied".to_string()],
        };

        let formatted = output.format::<true>(10, DEFAULT_MAX_LINE_LENGTH);

        assert!(formatted.contains("Partial results"));
    }

    #[test]
    fn collect_file_matches_reports_error_for_missing_file() {
        let temp = tempdir().unwrap();
        let missing = temp.path().join("missing.txt");
        let matcher = RegexMatcher::new("hello").unwrap();
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(0))
            .build();

        let result = collect_file_matches(&matcher, &mut searcher, &missing);

        assert!(result.matches.is_empty());
        assert!(result.error.is_some());
    }

    #[test]
    fn grep_marks_results_partial_when_walker_reports_error() {
        let temp = tempdir().unwrap();
        let missing_root = temp.path().join("missing-root");
        let resolver = AbsolutePathResolver;

        let result =
            grep_search(&resolver, "hello", None, missing_root.to_str().unwrap(), 10).unwrap();

        assert!(result.partial);
        assert_eq!(result.match_count, 0);
        assert!(!result.truncated);
        assert!(!result.errors.is_empty());
    }
}
