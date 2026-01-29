//! Benchmarks for agent parsing.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_coding_tools_subagents::AgentLoader;

/// Loads a real agent fixture file at runtime.
fn load_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/benches/fixtures/orchestrator-quality-gate-gpt5.md"
    ))
    .expect("failed to load fixture file")
}

fn benchmark_parse_frontmatter(c: &mut Criterion) {
    let real_lf = load_fixture();
    let real_crlf = real_lf.replace('\n', "\r\n");

    let mut group = c.benchmark_group("parse_frontmatter");
    group.throughput(Throughput::Bytes(real_lf.len() as u64));

    group.bench_with_input(
        BenchmarkId::new("real_agent", "lf"),
        &real_lf,
        |b, input| {
            b.iter(|| {
                black_box({
                    let mut loader = AgentLoader::new();
                    loader.add_from_str(black_box(input), "benchmark");
                    loader.load()
                })
            })
        },
    );

    group.bench_with_input(
        BenchmarkId::new("real_agent", "crlf"),
        &real_crlf,
        |b, input| {
            b.iter(|| {
                black_box({
                    let mut loader = AgentLoader::new();
                    loader.add_from_str(black_box(input), "benchmark");
                    loader.load()
                })
            })
        },
    );

    group.finish();
}

criterion_group!(benches, benchmark_parse_frontmatter);
criterion_main!(benches);
