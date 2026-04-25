//! Benchmarks for the `edit_file` tool.
//!
//! Tests exact string replacement performance across file sizes and scenarios
//! using real Rust source files as test data.
//!
//! # Benchmark Groups
//!
//! - `edit_file`: Core edit performance across sizes and match counts
//!
//! # Test Cases
//!
//! ```text
//! | Case             | Source    | Occurrences | replace_all | What it tests                      |
//! |------------------|-----------|-------------|-------------|------------------------------------|
//! | rename_string    | corpus S  | 1           | false       | Rename a string literal            |
//! | change_type      | corpus M  | 1           | false       | Change a type annotation           |
//! | update_constant  | corpus L  | 1           | false       | Update a numeric constant          |
//! | replace_all      | corpus L  | many        | true        | Bulk rename across many occurrences|
//! | not_found        | corpus M  | 0           | false       | Error path: string not found       |
//! ```
//!
//! # Running Benchmarks
//!
//! Quick run (1s per benchmark):
//! ```sh
//! cargo bench -p reloaded-code-core --no-default-features --features blocking --bench tools_edit -- --sample-size 10 --measurement-time 1 --warm-up-time 1
//! ```

#[path = "common/mod.rs"]
mod common;

use common::corpus_content;
use common::CorpusSize;
use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use reloaded_code_core::path::AbsolutePathResolver;
use reloaded_code_core::tools::{edit_file, EditRequest, EditSettings};
use tempfile::TempDir;

fn create_temp_file(content: &str) -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_input.rs");
    std::fs::write(&file_path, content).unwrap();
    (temp_dir, file_path.to_str().unwrap().to_owned())
}

fn bench_edit_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("edit_file");

    let resolver = AbsolutePathResolver;
    let settings = EditSettings::new();

    let content_small = corpus_content(CorpusSize::Small);
    let content_medium = corpus_content(CorpusSize::Medium);
    let content_large = corpus_content(CorpusSize::Large);

    // Case 1: rename_string - rename a string literal in a small file
    {
        let content = content_small;
        group.throughput(Throughput::Elements(1));
        group.bench_function("rename_string", |b| {
            b.iter_batched(
                || create_temp_file(content),
                |(_dir, path)| {
                    black_box(edit_file(
                        &resolver,
                        EditRequest {
                            file_path: path,
                            old_string: "update_plan".to_owned(),
                            new_string: "update_task_plan".to_owned(),
                            replace_all: false,
                        },
                        &settings,
                    ))
                },
                BatchSize::SmallInput,
            )
        });
    }

    // Case 2: change_type - change a type annotation in a medium file
    {
        let content = content_medium;
        group.throughput(Throughput::Elements(1));
        group.bench_function("change_type", |b| {
            b.iter_batched(
                || create_temp_file(content),
                |(_dir, path)| {
                    black_box(edit_file(
                        &resolver,
                        EditRequest {
                            file_path: path,
                            old_string: "mpsc::UnboundedSender<OutgoingMessage>".to_owned(),
                            new_string: "mpsc::Sender<OutgoingMessage>".to_owned(),
                            replace_all: false,
                        },
                        &settings,
                    ))
                },
                BatchSize::SmallInput,
            )
        });
    }

    // Case 3: update_constant - update a numeric constant in a large file
    {
        let content = content_large;
        group.throughput(Throughput::Elements(1));
        group.bench_function("update_constant", |b| {
            b.iter_batched(
                || create_temp_file(content),
                |(_dir, path)| {
                    black_box(edit_file(
                        &resolver,
                        EditRequest {
                            file_path: path,
                            old_string: "Duration::from_millis(100)".to_owned(),
                            new_string: "Duration::from_millis(200)".to_owned(),
                            replace_all: false,
                        },
                        &settings,
                    ))
                },
                BatchSize::SmallInput,
            )
        });
    }

    // Case 4: replace_all - bulk rename in a large file
    {
        let content = content_large;
        group.throughput(Throughput::Elements(1));
        group.bench_function("replace_all", |b| {
            b.iter_batched(
                || create_temp_file(content),
                |(_dir, path)| {
                    black_box(edit_file(
                        &resolver,
                        EditRequest {
                            file_path: path,
                            old_string: "process_id".to_owned(),
                            new_string: "session_id".to_owned(),
                            replace_all: true,
                        },
                        &settings,
                    ))
                },
                BatchSize::SmallInput,
            )
        });
    }

    // Case 5: not_found - old_string not in file (error path)
    {
        let content = content_medium;
        group.throughput(Throughput::Elements(1));
        group.bench_function("not_found", |b| {
            b.iter_batched(
                || create_temp_file(content),
                |(_dir, path)| {
                    black_box(edit_file(
                        &resolver,
                        EditRequest {
                            file_path: path,
                            old_string: "__NONEXISTENT_STRING_XYZ_12345__".to_owned(),
                            new_string: "replaced".to_owned(),
                            replace_all: false,
                        },
                        &settings,
                    ))
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

criterion_group!(benches, bench_edit_file);
criterion_main!(benches);
