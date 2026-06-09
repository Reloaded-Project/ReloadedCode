//! Event-style hooks — `on_run_start` fires before the run, `on_run_end`
//! fires after (even on error). No `RunHook` trait needed.
//!
//! Expected output:
//!   [on_run_start] agent=demo-agent, run_id=run-001, model=mock-model
//!   [MockExecutor] executing run for agent=demo-agent
//!   [on_run_end] agent=demo-agent, run_id=run-001, reason=Completed
//!   [Result] content=Run completed successfully., reason=Completed
//!
//! Run with:
//!   cargo run --example hooks-run-event -p reloaded-code-serdesai --features mock

use reloaded_code_core::{
    EndReason, HookRunContext, HookSet, RunConfig, RunHookFuture, RunOutput, RunUsage,
};

#[tokio::main]
async fn main() {
    // Build hook set using only event callbacks (no RunHook trait).
    let hooks = HookSet::builder()
        .on_run_start(|ctx: &HookRunContext<'_>| {
            println!(
                "[on_run_start] agent={}, run_id={}, model={}",
                ctx.agent_name, ctx.run_id, ctx.model_name
            );
        })
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
