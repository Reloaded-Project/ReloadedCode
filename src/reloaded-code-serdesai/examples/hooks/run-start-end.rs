//! Run hooks demo with mock models - shows `on_run_start`/`on_run_end`
//! convenience callbacks and `RunHook` intercept working together.
//!
//! Run with:
//!   cargo run --example hooks-run-start-end -p reloaded-code-serdesai --features mock

use reloaded_code_core::{
    EndReason, HookRunContext, HookSet, PreambleMessage, PreambleRole, RunConfig, RunHook,
    RunHookFuture, RunOriginal, RunOutput, RunUsage,
};

/// RunHook that adds a preamble message before the run.
struct PreambleInjector;

impl RunHook for PreambleInjector {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        mut config: RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!(
                "[PreambleInjector] injecting preamble for agent={}",
                ctx.agent_name
            );
            config.preamble_messages.push(PreambleMessage {
                role: PreambleRole::System,
                content: "You are a helpful assistant.".into(),
            });
            original.call(ctx, config).await
        })
    }
}

#[tokio::main]
async fn main() {
    // Build the hook set: on_run_start fires first, then PreambleInjector,
    // then on_run_end fires after the chain unwinds.
    let hooks = HookSet::builder()
        .on_run_start(|ctx: &HookRunContext<'_>| {
            println!(
                "[on_run_start] agent={}, run_id={}, model={}",
                ctx.agent_name, ctx.run_id, ctx.model_name
            );
        })
        .run_hook(PreambleInjector)
        .on_run_end(|ctx: &HookRunContext<'_>, reason: EndReason| {
            println!(
                "[on_run_end] agent={}, run_id={}, reason={:?}",
                ctx.agent_name, ctx.run_id, reason
            );
        })
        .build();

    let ctx = HookRunContext {
        agent_name: "demo-agent",
        run_id: "run-001",
        model_name: "mock-model",
    };

    // The RunExecutor wraps the actual agent run. Here we use a simple
    // mock executor for demonstration.
    struct MockExecutor;
    impl reloaded_code_core::RunExecutor for MockExecutor {
        fn execute<'a>(
            &'a self,
            ctx: &'a HookRunContext<'a>,
            _config: RunConfig,
        ) -> RunHookFuture<'a> {
            Box::pin(async move {
                println!("[MockExecutor] executing run for agent={}", ctx.agent_name);
                Ok(RunOutput {
                    content: "Run completed successfully.".into(),
                    reason: EndReason::Completed,
                    usage: RunUsage {
                        prompt_tokens: 100,
                        completion_tokens: 50,
                    },
                })
            })
        }
    }

    // Dispatch the run through the hook chain.
    let output = hooks
        .dispatch_run(&ctx, RunConfig::default(), &MockExecutor)
        .await
        .expect("run should succeed");

    println!(
        "[Result] content={}, reason={:?}",
        output.content, output.reason
    );
    println!(
        "[Result] usage: prompt={}, completion={}",
        output.usage.prompt_tokens, output.usage.completion_tokens
    );
}
