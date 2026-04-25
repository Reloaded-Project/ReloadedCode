//! Benchmarks for the `read_file` tool.
//!
//! Tests file reading performance across different file sizes, offsets,
//! line endings, and line-number modes.
//!
//! # Benchmark Groups
//!
//! - `read_file/with_line_numbers`: Read with `{n}: ` prefix on each line
//! - `read_file/without_line_numbers`: Read raw content without prefixes
//!
//! # Test Cases (per mode)
//!
//! ```text
//! | Case          | Source    | What it tests                     |
//! |---------------|-----------|-----------------------------------|
//! | small_file    | corpus S  | Small file, fits in one read      |
//! | medium_file   | corpus M  | Medium file, buffered reads       |
//! | large_file    | corpus L  | Large file, many lines processed  |
//! | offset_read   | corpus M  | Offset + limit, partial read      |
//! | crlf_file     | corpus M  | CRLF stripping overhead           |
//! ```
//!
//! # Running Benchmarks
//!
//! Quick run:
//! ```sh
//! cargo bench -p reloaded-code-core --no-default-features --features blocking --bench tools_read -- --sample-size 10 --measurement-time 1 --warm-up-time 1
//! ```

#[path = "common/mod.rs"]
mod common;

use common::{corpus_content, corpus_crlf, CorpusSize};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use reloaded_code_core::path::AbsolutePathResolver;
use reloaded_code_core::tools::{read_file, ReadRequest, ReadSettings};
use std::fs;
use tempfile::TempDir;

/// Holds a temporary test file for benchmarking.
///
/// The [`TempDir`] keeps the file alive until the struct is dropped.
struct TestFile {
    /// Temporary directory containing the test file.
    #[allow(dead_code)] // Used to keep temp dir alive (prevent drop)
    temp_dir: TempDir,
    /// Absolute path to the test file.
    path: String,
    /// Number of lines in the file content.
    line_count: usize,
}

/// Creates a temporary file with the given content for benchmarking.
fn create_test_file(content: &str) -> TestFile {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_input.rs");
    fs::write(&file_path, content).unwrap();
    let line_count = content.lines().count();
    TestFile {
        path: file_path.to_str().unwrap().to_owned(),
        temp_dir,
        line_count,
    }
}

/// Benchmarks `read_file` across file sizes, offsets, line endings, and line-number modes.
fn bench_read_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_file");

    // Setup: create test files with various corpus sizes
    let small = create_test_file(corpus_content(CorpusSize::Small));
    let medium = create_test_file(corpus_content(CorpusSize::Medium));
    let large = create_test_file(corpus_content(CorpusSize::Large));
    let crlf = create_test_file(&corpus_crlf(CorpusSize::Medium));

    // Setup: create resolver and settings variants
    let resolver = AbsolutePathResolver;
    let settings_ln = ReadSettings::new();
    let settings_no_ln = ReadSettings::new().with_line_numbers(false);
    let settings_offset_ln = ReadSettings::new().with_limits(50, 2000).unwrap();
    let settings_offset_no_ln = ReadSettings::new()
        .with_limits(50, 2000)
        .unwrap()
        .with_line_numbers(false);

    /// Test case configuration for a single benchmark run.
    struct Case {
        /// Identifier shown in benchmark output.
        name: &'static str,
        /// Path to the file to read.
        path: String,
        /// Starting line offset (1-indexed).
        offset: usize,
        /// Maximum lines to read, or `None` for entire file.
        limit: Option<usize>,
        /// Expected line count for throughput measurement.
        line_count: usize,
    }

    // Test cases: define scenarios to benchmark
    let cases: Vec<Case> = vec![
        Case {
            name: "small_file",
            path: small.path.clone(),
            offset: 1,
            limit: None,
            line_count: small.line_count,
        },
        Case {
            name: "medium_file",
            path: medium.path.clone(),
            offset: 1,
            limit: None,
            line_count: medium.line_count,
        },
        Case {
            name: "large_file",
            path: large.path.clone(),
            offset: 1,
            limit: None,
            line_count: large.line_count,
        },
        Case {
            name: "offset_read",
            path: medium.path.clone(),
            offset: 50,
            limit: Some(50),
            line_count: 50,
        },
        Case {
            name: "crlf_file",
            path: crlf.path.clone(),
            offset: 1,
            limit: None,
            line_count: crlf.line_count,
        },
    ];

    // Benchmark loop: run each case with and without line numbers
    for case in &cases {
        let settings_for_case_ln = if case.name == "offset_read" {
            settings_offset_ln.clone()
        } else {
            settings_ln.clone()
        };
        let settings_for_case_no_ln = if case.name == "offset_read" {
            settings_offset_no_ln.clone()
        } else {
            settings_no_ln.clone()
        };

        group.throughput(Throughput::Elements(case.line_count as u64));

        group.bench_with_input(
            BenchmarkId::new("with_line_numbers", case.name),
            &case.path,
            |b, path| {
                let s = &settings_for_case_ln;
                b.iter(|| {
                    read_file(
                        &resolver,
                        ReadRequest {
                            file_path: path.clone(),
                            offset: case.offset,
                            limit: case.limit,
                        },
                        s,
                    )
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("without_line_numbers", case.name),
            &case.path,
            |b, path| {
                let s = &settings_for_case_no_ln;
                b.iter(|| {
                    read_file(
                        &resolver,
                        ReadRequest {
                            file_path: path.clone(),
                            offset: case.offset,
                            limit: case.limit,
                        },
                        s,
                    )
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_read_file);
criterion_main!(benches);
