//! Glob pattern file matching operation.

use crate::error::{ToolError, ToolResult};
use crate::path::PathResolver;
use glob::Pattern;
use ignore::WalkBuilder;
use serde::Serialize;
use std::time::SystemTime;

const MAX_RESULTS: usize = 1000;

/// Output from glob file matching.
#[derive(Debug, Serialize)]
pub struct GlobOutput {
    /// Matched file paths relative to search directory, sorted by mtime (newest first).
    pub files: Vec<String>,
    /// Whether results were truncated due to limit.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

/// Finds files matching a glob pattern in the given directory.
///
/// Results are sorted by modification time (newest first) and respect `.gitignore`.
pub fn glob_files<R: PathResolver>(
    resolver: &R,
    pattern: &str,
    search_path: &str,
) -> ToolResult<GlobOutput> {
    let path = resolver.resolve(search_path)?;

    if !path.is_dir() {
        return Err(ToolError::InvalidPath(format!(
            "path is not a directory: {}",
            path.display()
        )));
    }

    let compiled_pattern =
        Pattern::new(pattern).map_err(|e| ToolError::InvalidPattern(e.to_string()))?;

    let mut files_with_mtime: Vec<(String, SystemTime)> = Vec::new();

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

        if let Some(ft) = entry.file_type() {
            if ft.is_dir() {
                continue;
            }
        } else {
            continue;
        }

        let rel_path = match entry.path().strip_prefix(&path) {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(_) => continue,
        };

        // Normalize Windows backslashes to forward slashes for glob pattern matching
        #[cfg(windows)]
        let rel_path = rel_path.replace('\\', "/");

        if rel_path.is_empty() {
            continue;
        }

        if !compiled_pattern.matches(&rel_path) {
            continue;
        }

        let mtime = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        files_with_mtime.push((rel_path, mtime));
    }

    files_with_mtime.sort_by(|a, b| b.1.cmp(&a.1));

    let truncated = files_with_mtime.len() > MAX_RESULTS;

    let files: Vec<String> = files_with_mtime
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(path, _)| path)
        .collect();

    Ok(GlobOutput { files, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::AbsolutePathResolver;
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

    #[test]
    fn glob_matches_pattern() {
        let dir = create_test_tree();
        let resolver = AbsolutePathResolver;
        let result = glob_files(&resolver, "**/*.rs", dir.path().to_str().unwrap()).unwrap();
        assert!(result.files.iter().any(|f| f.ends_with("lib.rs")));
    }

    #[test]
    fn glob_respects_gitignore() {
        let dir = create_test_tree();
        let resolver = AbsolutePathResolver;
        let result = glob_files(&resolver, "**/*", dir.path().to_str().unwrap()).unwrap();
        assert!(!result.files.iter().any(|f| f.contains("target")));
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

        let result = glob_files(&resolver, "**/*.txt", base.to_str().unwrap()).unwrap();

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

    #[test]
    fn glob_returns_forward_slash_paths() {
        // Patterns and returned paths use forward slashes on all platforms
        let dir = create_test_tree();
        let resolver = AbsolutePathResolver;
        let result = glob_files(&resolver, "**/*.rs", dir.path().to_str().unwrap()).unwrap();

        // Verify matching works with forward-slash patterns
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].ends_with("lib.rs"));

        // Verify returned paths use forward slashes (critical for Windows)
        for path in &result.files {
            assert!(!path.contains('\\'), "expected forward slashes: {path}");
        }
        assert!(result.files.iter().any(|f| f.contains('/')));
    }
}
