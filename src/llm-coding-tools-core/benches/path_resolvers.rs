//! Benchmarks for path resolver implementations.
//!
//! Tests performance on real filesystem paths using the current workspace.
//!
//! # Benchmark Groups
//!
//! - `resolvers`: Compares [`AllowedPathResolver`] and [`AllowedGlobResolver`] on the same paths
//! - `multiple_bases`: Tests [`AllowedPathResolver`] with multiple base directories
//! - `canonicalize`: Isolates `canonicalize` vs `soft_canonicalize` performance
//! - `external_directory`: Tests external directory permission fallback for absolute paths
//!
//! # Test Cases (resolvers)
//!
//! ```text
//! | Case                   | Path                                               | What it tests                                  |
//! |------------------------|----------------------------------------------------|------------------------------------------------|
//! | existing_file          | src/lib.rs                                         | Fast path: file exists, canonicalize succeeds  |
//! | new_file_existing_dir  | src/new_file_test.rs                               | Fast path: parent exists, canonicalize parent   |
//! | new_file_missing_dir   | src/new_dir/nested/new_file_test.rs                | Slow path: soft-canonicalize for non-existent  |
//! | policy_reject          | benchmarks/new_file_test.rs                        | Rejection via glob policy after resolution     |
//! | deep_nested            | src/llm-coding-tools-core/src/path/.../policy.rs   | Longer path, more components to process        |
//! | traversal_reject       | ../../../outside.txt                               | Early rejection via lexical escape check       |
//! ```
//!
//! # Reference Results (Linux, optimized build)
//!
//! ```text
//! resolvers/AllowedPathResolver/existing_file          ~2.1 µs
//! resolvers/AllowedPathResolver/new_file_existing_dir ~4.1 µs  (optimized: parent canonicalize)
//! resolvers/AllowedPathResolver/new_file_missing_dir  ~11.7 µs (fallback: soft_canonicalize)
//! resolvers/AllowedPathResolver/policy_reject         ~9.8 µs  (no policy, resolves normally)
//! resolvers/AllowedPathResolver/deep_nested           ~12.7 µs
//! resolvers/AllowedPathResolver/traversal_reject      ~21 ns
//!
//! resolvers/AllowedGlobResolver_simple_policy/existing_file          ~2.3 µs  (overhead: ~200 ns)
//! resolvers/AllowedGlobResolver_simple_policy/new_file_existing_dir  ~4.4 µs  (overhead: ~300 ns)
//! resolvers/AllowedGlobResolver_simple_policy/new_file_missing_dir   ~12.0 µs (overhead: ~300 ns)
//! resolvers/AllowedGlobResolver_simple_policy/policy_reject          ~10.0 µs (must resolve to check policy)
//! resolvers/AllowedGlobResolver_simple_policy/deep_nested            ~12.9 µs
//! resolvers/AllowedGlobResolver_simple_policy/traversal_reject       ~21 ns
//!
//! resolvers/AllowedGlobResolver_complex_policy/existing_file          ~2.6 µs
//! resolvers/AllowedGlobResolver_complex_policy/new_file_existing_dir  ~4.6 µs
//! resolvers/AllowedGlobResolver_complex_policy/new_file_missing_dir   ~12.2 µs
//! resolvers/AllowedGlobResolver_complex_policy/policy_reject          ~10.5 µs (must resolve to check policy)
//! resolvers/AllowedGlobResolver_complex_policy/deep_nested            ~13.2 µs
//! resolvers/AllowedGlobResolver_complex_policy/traversal_reject       ~21 ns
//!
//! multiple_bases/first_base    ~2.1 µs
//! multiple_bases/second_base   ~3.6 µs
//! multiple_bases/third_base    ~3.6 µs
//! multiple_bases/not_found     ~3.6 µs
//!
//! canonicalize/existing_file_canonicalize         ~1.9 µs
//! canonicalize/existing_file_soft_canonicalize    ~5.3 µs  (2.7x slower than canonicalize)
//! canonicalize/new_file_shallow_soft_canonicalize ~7.2 µs
//! canonicalize/new_file_deep_soft_canonicalize    ~8.4 µs
//!
//! external_directory/AllowedPathResolver/external_existing_file  ~548 ns  (canonicalize + permission)
//! external_directory/AllowedPathResolver/external_new_file       ~3.3 µs  (soft_canonicalize + permission)
//! external_directory/AllowedPathResolver/external_rejected       ~2.4 µs  (canonicalize + deny)
//! external_directory/AllowedPathResolver/external_no_ruleset     ~2.3 µs  (canonicalize, no permission)
//! external_directory/AllowedPathResolver/relative_still_fails    ~9.8 µs  (soft_canonicalize, not external)
//!
//! external_directory/AllowedGlobResolver/external_existing_file  ~535 ns  (canonicalize + permission)
//! external_directory/AllowedGlobResolver/external_new_file       ~3.3 µs  (soft_canonicalize + permission)
//! external_directory/AllowedGlobResolver/external_rejected       ~2.3 µs  (canonicalize + deny)
//! external_directory/AllowedGlobResolver/external_no_ruleset     ~2.3 µs  (canonicalize, no permission)
//! external_directory/AllowedGlobResolver/relative_still_fails    ~9.8 µs  (soft_canonicalize, not external)
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
use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};
use soft_canonicalize::soft_canonicalize;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

const EXISTING_FILE: &str = "src/lib.rs";
const NEW_FILE_EXISTING_DIR: &str = "src/new_file_test.rs";
// Path that matches simple policy (src/**/*.rs) but has missing directories
const NEW_FILE_MISSING_DIR: &str = "src/new_dir/nested/new_file_test.rs";
// Path that does NOT match simple policy - tests early rejection
const POLICY_REJECT: &str = "benchmarks/new_file_test.rs";
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
        ("policy_reject", POLICY_REJECT),
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

/// Benchmarks the external directory permission fallback path.
///
/// Tests the performance of resolving paths that are outside all base
/// directories but may be allowed via an `"external_directory"` permission rule.
///
/// # Test Cases
///
/// ```text
/// | Case                    | Description                                              |
/// |-------------------------|----------------------------------------------------------|
/// | external_existing_file  | Absolute path to file in allowed temp dir (fast path)    |
/// | external_new_file       | Absolute path to new file in allowed temp dir (soft_can) |
/// | external_rejected       | Absolute path denied by permission ruleset (early exit)  |
/// | external_no_ruleset     | Absolute path with no external_permission configured     |
/// | relative_still_fails    | Relative path even when external_permission is set       |
/// ```
fn bench_external_directory(c: &mut Criterion) {
    let mut group = c.benchmark_group("external_directory");

    let current_dir = std::env::current_dir().unwrap();
    let external_dir = TempDir::new().unwrap();
    let existing_file = external_dir.path().join("existing.txt");
    fs::write(&existing_file, "content").unwrap();
    let new_file = external_dir.path().join("subdir/new_file.txt");

    let canon_external = soft_canonicalize(external_dir.path()).unwrap();
    let allow_pattern = canon_external.join("*").to_str().unwrap().to_owned();

    let mut allow_ruleset = Ruleset::new();
    allow_ruleset.push(
        Rule::new(
            "external_directory",
            allow_pattern.as_str(),
            PermissionAction::Allow,
        )
        .unwrap(),
    );

    let mut deny_ruleset = Ruleset::new();
    deny_ruleset.push(Rule::new("external_directory", "*", PermissionAction::Deny).unwrap());

    let existing_file_str = existing_file.to_str().unwrap().to_owned();
    let new_file_str = new_file.to_str().unwrap().to_owned();
    let rejected_path = std::env::temp_dir()
        .join("rejected_external")
        .join("path.txt")
        .to_str()
        .unwrap()
        .to_owned();

    let allowed_path_resolver = AllowedPathResolver::new(vec![current_dir.clone()])
        .unwrap()
        .with_external_permission(Arc::new(allow_ruleset.clone()));

    let denied_path_resolver = AllowedPathResolver::new(vec![current_dir.clone()])
        .unwrap()
        .with_external_permission(Arc::new(deny_ruleset.clone()));

    let no_permission_resolver = AllowedPathResolver::new(vec![current_dir.clone()]).unwrap();

    let allowed_glob_resolver = AllowedGlobResolver::new(vec![current_dir.clone()])
        .unwrap()
        .with_external_permission(Arc::new(allow_ruleset));

    let denied_glob_resolver = AllowedGlobResolver::new(vec![current_dir.clone()])
        .unwrap()
        .with_external_permission(Arc::new(deny_ruleset));

    let no_permission_glob_resolver = AllowedGlobResolver::new(vec![current_dir.clone()]).unwrap();

    group.throughput(Throughput::Elements(1));

    for (case_name, path_input) in [
        ("external_existing_file", existing_file_str.as_str()),
        ("external_new_file", new_file_str.as_str()),
        ("external_rejected", rejected_path.as_str()),
        ("external_no_ruleset", rejected_path.as_str()),
        ("relative_still_fails", "relative/path.txt"),
    ] {
        let (path_resolver, glob_resolver) = match case_name {
            "external_existing_file" | "external_new_file" => {
                (&allowed_path_resolver, &allowed_glob_resolver)
            }
            "external_rejected" => (&denied_path_resolver, &denied_glob_resolver),
            "external_no_ruleset" => (&no_permission_resolver, &no_permission_glob_resolver),
            "relative_still_fails" => (&allowed_path_resolver, &allowed_glob_resolver),
            _ => unreachable!(),
        };

        group.bench_with_input(
            BenchmarkId::new("AllowedPathResolver", case_name),
            path_resolver,
            |b, resolver| b.iter(|| resolver.resolve(black_box(path_input))),
        );

        group.bench_with_input(
            BenchmarkId::new("AllowedGlobResolver", case_name),
            glob_resolver,
            |b, resolver| b.iter(|| resolver.resolve(black_box(path_input))),
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_resolvers_same_paths,
    bench_multiple_bases,
    bench_canonicalize_vs_soft,
    bench_external_directory
);

criterion_main!(benches);
