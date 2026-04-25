//! Benchmarks for the `write_file` tool.
//!
//! Tests file writing performance across different content sizes and
//! directory nesting depths using real Rust source files as test data.
//!
//! # Benchmark Groups
//!
//! - `write_file`: Core write performance across sizes and scenarios
//!
//! # Test Cases
//!
//! ```text
//! | Case          | Content   | Path depth | What it tests                         |
//! |---------------|-----------|------------|---------------------------------------|
//! | small_write   | corpus S  | 1          | Small write to flat directory         |
//! | medium_write  | corpus M  | 1          | Medium write to flat directory        |
//! | large_write   | corpus L  | 1          | Large write to flat directory         |
//! | nested_dirs   | corpus S  | 4 (a/b/c/) | Write creating nested directories    |
//! ```
//!
//! # Running Benchmarks
//!
//! Quick run (1s per benchmark):
//! ```sh
//! cargo bench -p reloaded-code-core --no-default-features --features blocking --bench tools_write -- --sample-size 10 --measurement-time 1 --warm-up-time 1
//! ```

#[path = "common/mod.rs"]
mod common;

use common::corpus_content;
use common::CorpusSize;
use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use reloaded_code_core::path::AbsolutePathResolver;
use reloaded_code_core::tools::{write_file, WriteRequest, WriteSettings};
use tempfile::TempDir;

fn bench_write_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("write_file");

    // Setup: resolver and settings
    let resolver = AbsolutePathResolver;
    let settings = WriteSettings::new();

    // Setup: load corpus content
    let small_content = corpus_content(CorpusSize::Small).to_owned();
    let medium_content = corpus_content(CorpusSize::Medium).to_owned();
    let large_content = corpus_content(CorpusSize::Large).to_owned();

    // Test cases definition
    let cases: Vec<(&str, String, &str)> = vec![
        ("small_write", small_content, "small.rs"),
        ("medium_write", medium_content, "medium.rs"),
        ("large_write", large_content, "large.rs"),
        (
            "nested_dirs",
            corpus_content(CorpusSize::Small).to_owned(),
            "a/b/c/deep.rs",
        ),
    ];

    // Benchmark loop
    for (case_name, content, file_name) in &cases {
        group.throughput(Throughput::Bytes(content.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("AbsolutePathResolver", case_name),
            content,
            |b, content| {
                b.iter_batched(
                    || {
                        let dir = TempDir::new().unwrap();
                        let path = dir.path().join(file_name);
                        (dir, path.to_str().unwrap().to_owned(), content.clone())
                    },
                    |(_dir, path, content)| {
                        black_box(write_file(
                            &resolver,
                            WriteRequest {
                                file_path: path,
                                content,
                            },
                            &settings,
                        ))
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_write_file);
criterion_main!(benches);
