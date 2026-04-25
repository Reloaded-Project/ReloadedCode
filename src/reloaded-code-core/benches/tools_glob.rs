//! Benchmarks for the `glob_files` tool.
//!
//! Tests glob matching performance across different directory tree sizes,
//! match densities, and nesting depths using real Rust source files as test data.
//!
//! # Benchmark Groups
//!
//! - `glob_files`: Core glob-walk performance across tree shapes
//!
//! # Test Cases
//!
//! ```text
//! | Case         | Files | Pattern      | Matches | What it tests                         |
//! |--------------|-------|--------------|---------|---------------------------------------|
//! | small_tree   | 8     | **/*.rs      | 5       | Small project, fast walk              |
//! | large_tree   | 300   | **/*.rs      | 150     | Large monorepo, many matches          |
//! | no_matches   | 300   | *.xyz        | 0       | Walk with no matches, full traversal  |
//! | deep_nesting | 10    | **/*.rs      | 10      | Deep directory nesting, path handling |
//! ```
//!
//! # Running Benchmarks
//!
//! Quick run (1s per benchmark):
//! ```sh
//! cargo bench -p reloaded-code-core --bench tools_glob -- --sample-size 10 --measurement-time 1 --warm-up-time 1
//! ```

#[path = "common/mod.rs"]
mod common;

use common::corpus_content;
use common::CorpusSize;
use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use reloaded_code_core::path::AbsolutePathResolver;
use reloaded_code_core::tools::{glob_files, GlobRequest, GlobSettings};
use std::fs;
use tempfile::TempDir;

/// Temporary directory fixture for benchmark tests.
///
/// Holds a temporary directory and its metadata for use in glob performance tests.
/// The [`TempDir`] is kept alive for the duration of the benchmark to prevent premature cleanup.
struct TreeFixture {
    /// Temporary directory that owns the test files.
    #[allow(dead_code)] // Used to keep temp dir alive (prevent drop)
    temp_dir: TempDir,
    /// Absolute path to the fixture root directory.
    path: String,
    /// Number of files in the fixture tree.
    file_count: usize,
}

/// Creates a small test fixture with typical Rust project structure.
///
/// Layout:
/// ```text
/// src/lib.rs, src/main.rs, src/utils/mod.rs
/// tests/integration.rs
/// benches/bench_main.rs
/// Cargo.toml, README.md, .gitignore
/// ```
fn create_small_tree() -> TreeFixture {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    let dirs = ["src", "src/utils", "tests", "benches"];
    for dir in &dirs {
        fs::create_dir_all(base.join(dir)).unwrap();
    }

    let files = [
        "src/lib.rs",
        "src/utils/mod.rs",
        "src/main.rs",
        "tests/integration.rs",
        "benches/bench_main.rs",
        "Cargo.toml",
        "README.md",
        ".gitignore",
    ];

    for file in &files {
        let content = corpus_content(CorpusSize::Small);
        fs::write(base.join(file), content).unwrap();
    }

    TreeFixture {
        path: base.to_str().unwrap().to_owned(),
        temp_dir,
        file_count: files.len(),
    }
}

/// Creates a large test fixture simulating a monorepo structure.
///
/// Layout:
/// ```text
/// src/module_{00-31}/file_{000-004}.{rs,txt}  (250 files: 5 per module, 50 modules)
/// root_{000-049}.toml                         (50 files)
/// ```
fn create_large_tree() -> TreeFixture {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    let mut count = 0;
    let sizes = [CorpusSize::Small, CorpusSize::Medium, CorpusSize::Large];

    // Create 50 modules, each with 5 files (mixed rs/txt extensions)
    for module in 0..50 {
        let dir = base.join(format!("src/module_{module:02x}"));
        fs::create_dir_all(&dir).unwrap();

        for file_idx in 0..5 {
            let size = sizes[file_idx % 3];
            let ext = if file_idx % 4 == 0 { "txt" } else { "rs" };
            fs::write(
                dir.join(format!("file_{file_idx:03}.{ext}")),
                corpus_content(size),
            )
            .unwrap();
            count += 1;
        }
    }

    // Create 50 root-level toml files
    for i in 0..50 {
        fs::write(
            base.join(format!("root_{i:03}.toml")),
            corpus_content(CorpusSize::Small),
        )
        .unwrap();
        count += 1;
    }

    TreeFixture {
        path: base.to_str().unwrap().to_owned(),
        temp_dir,
        file_count: count,
    }
}

/// Creates a deeply nested test fixture for path handling stress tests.
///
/// Layout (10 files):
/// ```text
/// level_0/data.rs
/// level_0/level_1/data.rs
/// level_0/.../level_9/data.rs
/// ```
fn create_deep_nesting_tree() -> TreeFixture {
    let temp_dir = TempDir::new().unwrap();
    let base = temp_dir.path();

    let mut current = base.to_path_buf();
    let mut count = 0;

    // Create 10 nested levels, each with a data.rs file
    for level in 0..10 {
        current = current.join(format!("level_{level}"));
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("data.rs"), corpus_content(CorpusSize::Small)).unwrap();
        count += 1;
    }

    TreeFixture {
        path: base.to_str().unwrap().to_owned(),
        temp_dir,
        file_count: count,
    }
}

/// Benchmarks [`glob_files`] performance across different tree shapes.
///
/// Tests four scenarios: small tree (8 files), large tree (300 files),
/// no-match traversal, and deep nesting (10 levels). Each case measures
/// glob matching throughput using [`AbsolutePathResolver`].
fn bench_glob_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_files");

    let small = create_small_tree();
    let large = create_large_tree();
    let deep = create_deep_nesting_tree();

    let resolver = AbsolutePathResolver;
    let settings = GlobSettings::new().with_limit(1000).unwrap();

    let cases: Vec<(&str, &str, &str, usize)> = vec![
        ("small_tree", &small.path, "**/*.rs", small.file_count),
        ("large_tree", &large.path, "**/*.rs", large.file_count),
        ("no_matches", &large.path, "*.xyz", large.file_count),
        ("deep_nesting", &deep.path, "**/*.rs", deep.file_count),
    ];

    for (case_name, path, pattern, file_count) in &cases {
        group.throughput(Throughput::Elements(*file_count as u64));
        group.bench_with_input(
            BenchmarkId::new("AbsolutePathResolver", *case_name),
            &(*path, *pattern),
            |b, &(path, pattern)| {
                b.iter(|| {
                    black_box(glob_files(
                        black_box(&resolver),
                        GlobRequest {
                            pattern: pattern.to_string(),
                            path: path.to_string(),
                        },
                        black_box(&settings),
                    ))
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_glob_files);
criterion_main!(benches);
