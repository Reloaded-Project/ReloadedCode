//! Benchmarks for agent parsing.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_coding_tools_agents::{AgentLoader, SubagentRegistry};

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

    for (name, input) in [("lf", &real_lf), ("crlf", &real_crlf)] {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("real_agent", "lf"),
            &real_lf,
            |b, input| {
                b.iter(|| {
                    black_box({
                        let loader = AgentLoader::new();
                        let mut registry = SubagentRegistry::new();
                        loader
                            .add_from_str(&mut registry, black_box(input), "benchmark")
                            .unwrap();
                        registry.len()
                    })
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_parse_frontmatter);
criterion_main!(benches);
