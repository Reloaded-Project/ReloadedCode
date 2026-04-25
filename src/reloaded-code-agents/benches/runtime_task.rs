//! Benchmarks for [`AgentRuntime`] task-delegation cache lookups.
//!
//! Measures the cost of [`AgentRuntime::allowed_tools`],
//! [`AgentRuntime::summarize_callable_targets`], and
//! [`AgentRuntime::can_delegate_to`] across varying agent counts.

use ahash::AHashMap;
use core::hint::black_box;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use indexmap::IndexMap;
use reloaded_code_agents::{
    AgentCatalog, AgentConfig, AgentMode, AgentRuntimeBuilder, AgentToolSettings, PermissionRule,
};
use reloaded_code_core::permissions::PermissionAction;
use reloaded_code_core::tool_metadata::{read as read_meta, task as task_meta};

/// Build a minimal [`AgentConfig`] for benchmark fixtures.
///
/// `permission` controls tool-access rules; all other fields are filled
/// with placeholder values suitable for performance measurement only.
fn build_agent(
    name: &str,
    mode: AgentMode,
    permission: IndexMap<String, PermissionRule>,
) -> AgentConfig {
    AgentConfig {
        name: name.into(),
        mode,
        description: format!("{name} description").into(),
        model: None,
        hidden: false,
        temperature: None,
        top_p: None,
        permission,
        options: AHashMap::new(),
        tool_settings: AgentToolSettings::default(),
        prompt: Default::default(),
    }
}

/// Create a permission map that denies all tools by default, but allows
/// pattern-matched delegation to agents named `review-*` or `worker-*`
/// via the task tool, and blanket-allows the read tool.
fn patterned_task_permission() -> IndexMap<String, PermissionRule> {
    let mut patterns = IndexMap::new();
    patterns.insert("*".to_string(), PermissionAction::Deny);
    patterns.insert("review-*".to_string(), PermissionAction::Allow);
    patterns.insert("worker-*".to_string(), PermissionAction::Allow);

    IndexMap::from([
        (task_meta::NAME.into(), PermissionRule::Pattern(patterns)),
        (
            read_meta::NAME.into(),
            PermissionRule::Action(PermissionAction::Allow),
        ),
    ])
}

/// Build an [`AgentRuntime`] with one `caller` primary agent and
/// `agent_count` subordinate agents.
///
/// Subordinate names cycle through `review-NNN`, `worker-NNN`, and
/// `misc-NNN` prefixes. Every 11th subordinate is a primary-mode agent;
/// the rest are subagents.
fn build_runtime(agent_count: usize) -> reloaded_code_agents::AgentRuntime {
    let mut agents = Vec::with_capacity(agent_count + 1);
    agents.push(build_agent(
        "caller",
        AgentMode::Primary,
        patterned_task_permission(),
    ));

    for idx in 0..agent_count {
        let name = match idx % 3 {
            0 => format!("review-{idx:03}"),
            1 => format!("worker-{idx:03}"),
            _ => format!("misc-{idx:03}"),
        };
        let mode = if idx % 11 == 0 {
            AgentMode::Primary
        } else {
            AgentMode::Subagent
        };
        agents.push(build_agent(&name, mode, IndexMap::new()));
    }

    AgentRuntimeBuilder::new()
        .catalog(AgentCatalog::from_entries(agents))
        .build()
        .expect("benchmark fixture should not fail pattern expansion")
}

/// Benchmark cached delegation queries against runtimes of 16, 64, and 256 agents.
///
/// Measures four operations:
/// - **allowed_tools** – full tool-set resolution for the `caller` agent.
/// - **summaries** – callable-target summary strings for `caller`.
/// - **can_delegate_hit** – pattern-match hit (`caller` → `review-003`).
/// - **can_delegate_miss** – pattern-match miss (`caller` → `misc-002`).
fn bench_runtime_task_caches(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime/task_caches");

    for agent_count in [16_usize, 64, 256] {
        let runtime = build_runtime(agent_count);
        group.throughput(Throughput::Elements(1));

        group.bench_with_input(
            BenchmarkId::new("allowed_tools", agent_count),
            &runtime,
            |b, runtime| b.iter(|| black_box(runtime.allowed_tools("caller"))),
        );

        group.bench_with_input(
            BenchmarkId::new("summaries", agent_count),
            &runtime,
            |b, runtime| b.iter(|| black_box(runtime.summarize_callable_targets("caller"))),
        );

        group.bench_with_input(
            BenchmarkId::new("can_delegate_hit", agent_count),
            &runtime,
            |b, runtime| b.iter(|| black_box(runtime.can_delegate_to("caller", "review-003"))),
        );

        group.bench_with_input(
            BenchmarkId::new("can_delegate_miss", agent_count),
            &runtime,
            |b, runtime| b.iter(|| black_box(runtime.can_delegate_to("caller", "misc-002"))),
        );
    }

    group.finish();
}

criterion_group!(benches, bench_runtime_task_caches);
criterion_main!(benches);
