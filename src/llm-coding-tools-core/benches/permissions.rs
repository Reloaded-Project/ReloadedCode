//! Benchmarks for permission rule evaluation on the hot path.
//!
//! These benches focus on the checks that run on every gated tool call:
//! [`Ruleset::evaluate`] and [`OptionRulesetExt::check`].
//! Cases cover exact matches, wildcard permission keys, wildcard subject
//! patterns, and longer rulesets where the winning rule is near the end.

use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use llm_coding_tools_core::permissions::{PermissionAction, Rule, Ruleset};
use llm_coding_tools_core::permissions_ext::OptionRulesetExt;

/// A single benchmark scenario for permission rule evaluation.
///
/// Each case pairs a tool name and subject path with a pre-built [`Ruleset`]
/// so that benches can iterate without constructing fixtures on every sample.
struct PermissionCase {
    name: &'static str,
    tool_name: &'static str,
    subject: &'static str,
    ruleset: Ruleset,
}

/// Build a [`Ruleset`] with `rule_count - 1` deny rules followed by one
/// allow rule, so that the winning rule is always the last entry.
///
/// # Arguments
/// - `rule_count`: Total number of rules. Must be at least 1.
/// - `final_permission`: Tool-name pattern for the last (allow) rule.
/// - `final_pattern`: Subject pattern for the last (allow) rule.
fn build_ruleset(
    rule_count: usize,
    final_permission: impl Into<Box<str>>,
    final_pattern: impl Into<Box<str>>,
) -> Ruleset {
    assert!(rule_count >= 1, "rule_count must be >= 1");
    let mut ruleset = Ruleset::with_capacity(rule_count);

    for idx in 0..(rule_count - 1) {
        ruleset.push(
            Rule::new(
                format!("tool-{idx}"),
                format!("/workspace/other-{idx}.txt"),
                PermissionAction::Deny,
            )
            .expect("benchmark fixture patterns should not fail expansion"),
        );
    }

    ruleset.push(
        Rule::new(final_permission, final_pattern, PermissionAction::Allow)
            .expect("benchmark fixture patterns should not fail expansion"),
    );
    ruleset
}

/// Return the standard set of permission benchmark cases covering exact
/// matches at three rule-set sizes (1, 32, and 128 rules), plus wildcard
/// subject, wildcard permission key, and combined wildcard cases at
/// 32 rules and a combined wildcard case at 128 rules.
fn benchmark_cases() -> Vec<PermissionCase> {
    vec![
        PermissionCase {
            name: "exact_1_rule",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(1, "read", "/workspace/src/lib.rs"),
        },
        PermissionCase {
            name: "exact_32_rules",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(32, "read", "/workspace/src/lib.rs"),
        },
        PermissionCase {
            name: "exact_128_rules",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(128, "read", "/workspace/src/lib.rs"),
        },
        PermissionCase {
            name: "wildcard_subject_32_rules",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(32, "read", "/workspace/src/*.rs"),
        },
        PermissionCase {
            name: "wildcard_permission_32_rules",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(32, "re?d", "/workspace/src/lib.rs"),
        },
        PermissionCase {
            name: "wildcard_both_32_rules",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(32, "r*d", "/workspace/src/*.rs"),
        },
        PermissionCase {
            name: "wildcard_both_128_rules",
            tool_name: "read",
            subject: "/workspace/src/lib.rs",
            ruleset: build_ruleset(128, "r*d", "/workspace/src/*.rs"),
        },
    ]
}

/// Benchmark [`Ruleset::evaluate`] across all [`benchmark_cases`].
fn bench_ruleset_evaluate(c: &mut Criterion) {
    let mut group = c.benchmark_group("permissions/evaluate");
    let cases = benchmark_cases();

    group.throughput(Throughput::Elements(1));

    for case in &cases {
        group.bench_with_input(BenchmarkId::new("ruleset", case.name), case, |b, case| {
            b.iter(|| {
                black_box(
                    case.ruleset
                        .evaluate(black_box(case.tool_name), black_box(case.subject)),
                )
            })
        });
    }

    group.finish();
}

/// Benchmark [`OptionRulesetExt::check`] (ruleset lookup plus optional default
/// fallthrough) across all [`benchmark_cases`].
fn bench_check_permission(c: &mut Criterion) {
    let mut group = c.benchmark_group("permissions/check_permission");
    let cases = benchmark_cases();

    group.throughput(Throughput::Elements(1));

    for case in &cases {
        group.bench_with_input(BenchmarkId::new("ruleset", case.name), case, |b, case| {
            b.iter(|| {
                Some(black_box(&case.ruleset))
                    .check(black_box(case.tool_name), black_box(case.subject))
                    .expect("benchmark fixture should be allowed");
                black_box(())
            })
        });
    }

    group.finish();
}

criterion_group!(benches, bench_ruleset_evaluate, bench_check_permission);
criterion_main!(benches);
