//! Benchmarks for path resolver implementations.
//!
//! Tests performance on real filesystem paths using the current workspace.
//!
//! # Benchmark Groups
//!
//! - `resolvers`: Compares [`AllowedPathResolver`] and [`AllowedGlobResolver`] on the same paths
//! - `multiple_bases`: Tests [`AllowedPathResolver`] with multiple base directories
//! - `canonicalize`: Isolates `canonicalize` vs `soft_canonicalize` performance
//!
//! # Test Cases
//!
//! ```text
//! | Case                   | Path                                               | What it tests                                  |
//! |------------------------|----------------------------------------------------|------------------------------------------------|
//! | existing_file          | src/lib.rs                                         | Fast path: file exists, canonicalize succeeds  |
//! | new_file_existing_dir  | src/new_file_test.rs                               | Fast path: parent exists, canonicalize parent   |
//! | new_file_missing_dir   | benchmarks/new_file_test.rs                        | Slow path: soft-canonicalize for non-existent  |
//! | deep_nested            | src/llm-coding-tools-core/src/path/.../policy.rs   | Longer path, more components to process        |
//! | traversal_reject       | ../../../outside.txt                               | Early rejection via lexical escape check       |
//! ```
//!
//! # Reference Results (Linux, optimized build)
//!
//! ```text
//! resolvers/AllowedPathResolver/existing_file          ~1.8 µs
//! resolvers/AllowedPathResolver/new_file_existing_dir ~3.5 µs  (optimized: parent canonicalize)
//! resolvers/AllowedPathResolver/new_file_missing_dir  ~8.1 µs  (fallback: soft_canonicalize)
//! resolvers/AllowedPathResolver/deep_nested           ~10.5 µs
//! resolvers/AllowedPathResolver/traversal_reject      ~20 ns
//!
//! resolvers/AllowedGlobResolver_simple_policy/existing_file          ~2.0 µs
//! resolvers/AllowedGlobResolver_simple_policy/new_file_existing_dir  ~3.8 µs
//! resolvers/AllowedGlobResolver_simple_policy/new_file_missing_dir   ~8.4 µs
//! resolvers/AllowedGlobResolver_simple_policy/deep_nested            ~10.8 µs
//! resolvers/AllowedGlobResolver_simple_policy/traversal_reject       ~20 ns
//!
//! canonicalize/existing_file_canonicalize         ~1.6 µs
//! canonicalize/existing_file_soft_canonicalize    ~4.4 µs  (2.7x slower than canonicalize)
//! canonicalize/new_file_shallow_soft_canonicalize ~6.0 µs
//! canonicalize/new_file_deep_soft_canonicalize    ~7.0 µs
//! ```
//!
//! # Platform Differences
//!
//! On Unix, new files in existing directories use the fast path (canonicalize parent + join filename).
//! On Windows, the fast path uses `soft_canonicalize` due to complex path semantics.
//!
//! # Running Benchmarks
//!
//! Quick run (1s per benchmark):
//! ```sh
//! cargo bench -p llm-coding-tools-core --bench path_resolvers -- --sample-size 10 --measurement-time 1 --warm-up-time 1
//! ```
//!
//! Full run with baseline comparison:
//! ```sh
//! cargo bench -p llm-coding-tools-core --bench path_resolvers -- --save-baseline main
//! # make changes, then:
//! cargo bench -p llm-coding-tools-core --bench path_resolvers -- --baseline main
//! ```

use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_coding_tools_core::path::{
    AllowedGlobResolver, AllowedPathResolver, GlobPolicy, GlobPolicyBuilder, PathResolver,
};
use soft_canonicalize::soft_canonicalize;
use std::fs;
use tempfile::TempDir;

const EXISTING_FILE: &str = "src/lib.rs";
const NEW_FILE_EXISTING_DIR: &str = "src/new_file_test.rs";
const NEW_FILE_MISSING_DIR: &str = "benchmarks/new_file_test.rs";
const DEEP_NESTED: &str = "src/llm-coding-tools-core/src/path/allowed_glob/policy.rs";
const TRAVERSAL: &str = "../../../outside.txt";

fn build_policy<F>(f: F) -> llm_coding_tools_core::error::ToolResult<GlobPolicy>
where
    F: FnOnce(GlobPolicyBuilder) -> llm_coding_tools_core::error::ToolResult<GlobPolicyBuilder>,
{
    f(GlobPolicy::builder()).and_then(|b| b.build())
}

/// Benchmarks [`AllowedPathResolver`] and [`AllowedGlobResolver`] on the same paths.
///
/// This group measures the core resolve operation under different conditions.
///
/// # Resolvers Compared
///
/// ```text
/// | Resolver                        | Description                              |
/// |---------------------------------|------------------------------------------|
/// | AllowedPathResolver             | Baseline: no glob policy                 |
/// | AllowedGlobResolver_simple      | Single rule: src/**/*.rs                 |
/// | AllowedGlobResolver_complex     | 10 rules: realistic project config       |
/// ```
///
/// # Expected Performance (Unix)
///
/// ```text
/// | Case                   | Expected Time | Why                                    |
/// |------------------------|---------------|----------------------------------------|
/// | existing_file          | 1-2 µs        | canonicalize is fast for existing      |
/// | new_file_existing_dir  | 3-4 µs        | canonicalize parent, join filename     |
/// | new_file_missing_dir   | 7-8 µs        | soft-canonicalize walks filesystem     |
/// | deep_nested            | 10-11 µs      | more path components to process        |
/// | traversal_reject       | ~20 ns        | lexical check only, no filesystem I/O  |
/// ```
fn bench_resolvers_same_paths(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolvers");

    let current_dir = std::env::current_dir().unwrap();

    // Baseline: AllowedPathResolver (no glob policy)
    let allowed = AllowedPathResolver::new(vec![current_dir.clone()]).unwrap();

    // Simple policy: single glob pattern (src/**/*.rs)
    // This tests minimal glob matching overhead.
    let simple_policy = build_policy(|b| b.allow("src/**/*.rs")).unwrap();

    // Complex policy: 10 rules simulating a realistic project configuration.
    // Tests last-match-wins semantics and rule iteration overhead.
    let complex_policy = build_policy(|b| {
        b.allow("src/**/*.rs")?
            .deny("target/**")?
            .allow("*.toml")?
            .deny("*.log")?
            .allow("benches/**")?
            .deny("**/test_data/**")?
            .allow("tests/**/*.rs")?
            .deny("node_modules/**")?
            .allow("examples/**")
    })
    .unwrap();

    let glob_simple = AllowedGlobResolver::new(vec![current_dir.clone()])
        .unwrap()
        .with_policy(simple_policy);
    let glob_complex = AllowedGlobResolver::new(vec![current_dir.clone()])
        .unwrap()
        .with_policy(complex_policy);

    group.throughput(Throughput::Elements(1));

    for (case_name, path_input) in [
        ("existing_file", EXISTING_FILE),
        ("new_file_existing_dir", NEW_FILE_EXISTING_DIR),
        ("new_file_missing_dir", NEW_FILE_MISSING_DIR),
        ("deep_nested", DEEP_NESTED),
        ("traversal_reject", TRAVERSAL),
    ] {
        group.bench_with_input(
            BenchmarkId::new("AllowedPathResolver", case_name),
            &allowed,
            |b, resolver| b.iter(|| resolver.resolve(black_box(path_input))),
        );

        group.bench_with_input(
            BenchmarkId::new("AllowedGlobResolver_simple_policy", case_name),
            &glob_simple,
            |b, resolver| b.iter(|| resolver.resolve(black_box(path_input))),
        );

        group.bench_with_input(
            BenchmarkId::new("AllowedGlobResolver_complex_policy", case_name),
            &glob_complex,
            |b, resolver| b.iter(|| resolver.resolve(black_box(path_input))),
        );
    }

    group.finish();
}

/// Benchmarks [`AllowedPathResolver`] with multiple base directories.
///
/// Tests how the resolver performs when it must search through multiple
/// allowed directories to find a match.
///
/// # Setup
///
/// ```text
/// | Base    | Directory         | Contains     |
/// |---------|-------------------|--------------|
/// | Base 1  | Current workspace | src/lib.rs   |
/// | Base 2  | Temp directory 1  | file1.txt    |
/// | Base 3  | Temp directory 2  | file2.txt    |
/// ```
///
/// # Test Cases
///
/// ```text
/// | Case        | What it tests                              |
/// |-------------|--------------------------------------------|
/// | first_base  | Path found in first base (fastest)          |
/// | second_base | Path found in second base (one miss, hit)   |
/// | third_base  | Path found in third base (two misses, hit)  |
/// | not_found   | Path not in any base (all bases tried)     |
/// ```
fn bench_multiple_bases(c: &mut Criterion) {
    let mut group = c.benchmark_group("multiple_bases");

    let current_dir = std::env::current_dir().unwrap();
    let temp1 = TempDir::new().unwrap();
    let temp2 = TempDir::new().unwrap();

    fs::write(temp1.path().join("file1.txt"), "content").unwrap();
    fs::write(temp2.path().join("file2.txt"), "content").unwrap();

    let resolver = AllowedPathResolver::new(vec![
        current_dir.clone(),
        temp1.path().to_path_buf(),
        temp2.path().to_path_buf(),
    ])
    .unwrap();

    group.throughput(Throughput::Elements(1));

    for (case_name, path_input) in [
        ("first_base", "src/lib.rs"),
        ("second_base", "file1.txt"),
        ("third_base", "file2.txt"),
        ("not_found", "nonexistent.xyz"),
    ] {
        group.bench_function(case_name, |b| {
            b.iter(|| resolver.resolve(black_box(path_input)))
        });
    }

    group.finish();
}

/// Benchmarks `std::fs::canonicalize` vs `soft_canonicalize` directly.
///
/// Isolates the core filesystem operation to understand where time is spent.
///
/// # Test Cases
///
/// ```text
/// | Case              | Path                          | canonicalize | soft_canonicalize |
/// |-------------------|-------------------------------|--------------|-------------------|
/// | existing_file     | src/lib.rs (exists)           | O(1) FS call | O(1) FS call      |
/// | new_file_shallow  | new_file.rs (in root)         | N/A          | O(1) FS call      |
/// | new_file_deep     | a/b/c/new_file.rs (3 levels)  | N/A          | O(4) FS calls     |
/// ```
fn bench_canonicalize_vs_soft(c: &mut Criterion) {
    let mut group = c.benchmark_group("canonicalize");

    let current_dir = std::env::current_dir().unwrap();
    let existing = current_dir.join("src/lib.rs");
    let new_shallow = current_dir.join("new_file.rs");
    let new_deep = current_dir.join("a/b/c/new_file.rs");

    group.throughput(Throughput::Elements(1));

    group.bench_function("existing_file_canonicalize", |b| {
        b.iter(|| existing.canonicalize().unwrap())
    });

    group.bench_function("existing_file_soft_canonicalize", |b| {
        b.iter(|| soft_canonicalize(&existing).unwrap())
    });

    group.bench_function("new_file_shallow_soft_canonicalize", |b| {
        b.iter(|| soft_canonicalize(&new_shallow).unwrap())
    });

    group.bench_function("new_file_deep_soft_canonicalize", |b| {
        b.iter(|| soft_canonicalize(&new_deep).unwrap())
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_resolvers_same_paths,
    bench_multiple_bases,
    bench_canonicalize_vs_soft
);

criterion_main!(benches);
