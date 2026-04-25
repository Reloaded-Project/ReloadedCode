//! Benchmarks for the `grep_search` tool.
//!
//! Tests grep performance across different file layouts, pattern complexity,
//! match densities, and line-number formatting modes using real Rust source
//! files as test data.
//!
//! # Benchmark Groups
//!
//! - `grep_search`: Core search + format with/without line numbers
//! - `grep_format`: Formatting only (search result reuse) with/without line numbers
//!
//! # Test Cases
//!
//! ```text
//! | Case            | Files | Pattern       | What it tests                    |
//! |-----------------|-------|---------------|----------------------------------|
//! | single_file     | 1     | fn            | Single file, many matches        |
//! | multi_file      | 10    | fn            | Multi-file, moderate matches     |
//! | no_matches      | 10    | xyznonexistent| No matches, fast rejection        |
//! | regex_pattern   | 10    | fn\s+\w+      | Complex regex matching           |
//! | large_tree      | 30    | fn            | Large directory tree traversal    |
//! ```
//!
//! # Running Benchmarks
//!
//! Quick run:
//! ```sh
//! cargo bench -p reloaded-code-core --bench tools_grep -- --sample-size 10 --measurement-time 1 --warm-up-time 1
//! ```

#[path = "common/mod.rs"]
mod common;

use common::{corpus_content, CorpusSize};
use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use reloaded_code_core::path::AbsolutePathResolver;
use reloaded_code_core::tools::{grep_search, GrepFormattingSettings, GrepRequest, GrepSettings};
use std::fs;
use tempfile::TempDir;

/// Holds a test directory with precomputed match counts for benchmarking.
struct TestDir {
    #[allow(dead_code)] // Used to keep temp dir alive (prevent drop)
    temp_dir: TempDir,
    path: String,
    total_matches: usize,
}

/// Creates a single-file test fixture for benchmarking single-file grep performance.
///
/// Layout:
/// ```text
/// plan.rs (small corpus)
/// ```
fn create_single_file() -> TestDir {
    let temp_dir = TempDir::new().unwrap();
    let content = corpus_content(CorpusSize::Small);
    fs::write(temp_dir.path().join("plan.rs"), content).unwrap();
    let matches = content.lines().filter(|l| l.contains("fn ")).count();
    TestDir {
        path: temp_dir.path().to_str().unwrap().to_owned(),
        temp_dir,
        total_matches: matches,
    }
}

/// Creates a test fixture with 10 Rust files cycling through corpus sizes.
///
/// Layout:
/// ```text
/// {prefix}0.rs (small corpus)
/// {prefix}1.rs (medium corpus)
/// {prefix}2.rs (large corpus)
/// ... (10 files total, cycling small/medium/large)
/// ```
fn create_test_files(prefix: &str) -> TestDir {
    let temp_dir = TempDir::new().unwrap();
    let mut total_matches = 0;
    for (i, size) in [CorpusSize::Small, CorpusSize::Medium, CorpusSize::Large]
        .iter()
        .cycle()
        .enumerate()
        .take(10)
    {
        let content = corpus_content(*size);
        let name = format!("{prefix}{i}.rs");
        fs::write(temp_dir.path().join(name), content).unwrap();
        total_matches += content.lines().filter(|l| l.contains("fn ")).count();
    }
    TestDir {
        path: temp_dir.path().to_str().unwrap().to_owned(),
        temp_dir,
        total_matches,
    }
}

fn create_multi_file() -> TestDir {
    create_test_files("file_")
}

fn create_no_matches() -> TestDir {
    let mut dir = create_test_files("nomatch_");
    dir.total_matches = 0;
    dir
}

fn create_regex_pattern() -> TestDir {
    create_test_files("src_")
}

/// Creates a test fixture with nested directories for benchmarking large tree traversal.
///
/// Layout:
/// ```text
/// src/module_00/mod_0.rs (small corpus)
/// src/module_01/mod_1.rs (medium corpus)
/// src/module_02/mod_2.rs (large corpus)
/// ... (30 modules total, cycling small/medium/large)
/// ```
fn create_large_tree() -> TestDir {
    let temp_dir = TempDir::new().unwrap();
    let mut total_matches = 0;
    let sizes = [CorpusSize::Small, CorpusSize::Medium, CorpusSize::Large];
    for i in 0..30 {
        let size = sizes[i % 3];
        let content = corpus_content(size);
        let dir = temp_dir.path().join(format!("src/module_{i:02x}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("mod_{i}.rs")), content).unwrap();
        total_matches += content.lines().filter(|l| l.contains("fn ")).count();
    }
    TestDir {
        path: temp_dir.path().to_str().unwrap().to_owned(),
        temp_dir,
        total_matches,
    }
}

/// Benchmarks `grep_search` with formatting across different test cases.
fn bench_grep_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("grep_search");

    // Setup: create test directories
    let single = create_single_file();
    let multi = create_multi_file();
    let no_matches = create_no_matches();
    let regex_case = create_regex_pattern();
    let large = create_large_tree();

    // Setup: shared resolver and settings
    let resolver = AbsolutePathResolver;
    let settings = GrepSettings::new().with_max_limit(1000).unwrap();

    // Test cases: (name, directory, pattern, expected match count)
    let cases: Vec<(&str, &TestDir, &str, usize)> = vec![
        ("single_file", &single, "fn ", single.total_matches),
        ("multi_file", &multi, "fn ", multi.total_matches),
        (
            "no_matches",
            &no_matches,
            "xyznonexistent",
            no_matches.total_matches,
        ),
        (
            "regex_pattern",
            &regex_case,
            r"fn\s+\w+",
            regex_case.total_matches,
        ),
        ("large_tree", &large, "fn ", large.total_matches),
    ];

    // Benchmark each case with and without line numbers
    for (case_name, test_dir, pattern, expected_matches) in &cases {
        group.throughput(Throughput::Elements(*expected_matches as u64));
        group.bench_with_input(
            BenchmarkId::new("with_line_numbers", case_name),
            pattern,
            |b, pat| {
                let request = GrepRequest {
                    pattern: pat.to_string(),
                    path: test_dir.path.clone(),
                    include: None,
                    limit: None,
                };
                b.iter(|| {
                    let result = grep_search(
                        black_box(&resolver),
                        GrepRequest {
                            pattern: request.pattern.clone(),
                            path: request.path.clone(),
                            include: request.include.clone(),
                            limit: request.limit,
                        },
                        black_box(&settings),
                    )
                    .unwrap();
                    black_box(result.format(GrepFormattingSettings::new()))
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("without_line_numbers", case_name),
            pattern,
            |b, pat| {
                let request = GrepRequest {
                    pattern: pat.to_string(),
                    path: test_dir.path.clone(),
                    include: None,
                    limit: None,
                };
                b.iter(|| {
                    let result = grep_search(
                        black_box(&resolver),
                        GrepRequest {
                            pattern: request.pattern.clone(),
                            path: request.path.clone(),
                            include: request.include.clone(),
                            limit: request.limit,
                        },
                        black_box(&settings),
                    )
                    .unwrap();
                    black_box(result.format(GrepFormattingSettings::new().with_line_numbers(false)))
                })
            },
        );
    }

    group.finish();
}

/// Benchmarks formatting of precomputed search results (isolates formatting overhead from search).
fn bench_grep_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("grep_format");

    let large = create_large_tree();
    let resolver = AbsolutePathResolver;
    let settings = GrepSettings::new().with_max_limit(1000).unwrap();

    let search_result = grep_search(
        &resolver,
        GrepRequest {
            pattern: "fn ".to_string(),
            path: large.path.clone(),
            include: None,
            limit: None,
        },
        &settings,
    )
    .unwrap();

    group.throughput(Throughput::Elements(search_result.match_count as u64));
    group.bench_function("with_line_numbers", |b| {
        b.iter(|| black_box(search_result.format(GrepFormattingSettings::new())))
    });
    group.bench_function("without_line_numbers", |b| {
        b.iter(|| {
            black_box(search_result.format(GrepFormattingSettings::new().with_line_numbers(false)))
        })
    });

    group.finish();
}

criterion_group!(benches, bench_grep_search, bench_grep_format);
criterion_main!(benches);
