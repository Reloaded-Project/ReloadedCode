//! Grep content search operation.

use crate::error::{ToolError, ToolResult};
use crate::path::PathResolver;
use glob::Pattern;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::WalkBuilder;
use serde::Serialize;
use std::path::Path;
use std::time::SystemTime;

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

    let glob_pattern = include
        .map(|g| Pattern::new(g).map_err(|e| ToolError::InvalidPattern(e.to_string())))
        .transpose()?;

    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let mut files: Vec<GrepFileMatches> = Vec::with_capacity(64);

    let walker = WalkBuilder::new(&path)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry_result in walker {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        match entry.file_type() {
            Some(ft) if ft.is_file() => {}
            _ => continue,
        }

        let entry_path = entry.path();

        if let Some(ref glob) = glob_pattern {
            let file_name = match entry_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };
            if !glob.matches(file_name) {
                continue;
            }
        }

        let matches = collect_file_matches(&matcher, &mut searcher, entry_path);
        if matches.is_empty() {
            continue;
        }

        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        files.push(GrepFileMatches {
            path: entry_path.to_string_lossy().into_owned(),
            matches,
            mtime,
        });
    }

    files.sort_by(|a, b| b.mtime.cmp(&a.mtime));

    let mut match_count = 0;
    let mut truncate_at = files.len();
    let mut truncated = false;

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
    })
}

fn collect_file_matches(
    matcher: &RegexMatcher,
    searcher: &mut Searcher,
    path: &Path,
) -> Vec<GrepLineMatch> {
    let mut matches = Vec::new();

    let _ = searcher.search_path(
        matcher,
        path,
        UTF8(|line_num, line| {
            matches.push(GrepLineMatch {
                line_num,
                line_text: line.trim_end().to_string(),
            });
            Ok(true)
        }),
    );

    matches
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
}
