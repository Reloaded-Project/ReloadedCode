//! Single `RunHook` — intercept run config, modify preamble, call original.
//!
//! Expected output:
//!   [PreambleInjector] injecting preamble for agent=demo-agent
//!   [MockExecutor] executing run for agent=demo-agent
//!   [Result] content=Run completed successfully., reason=Completed
//!
//! Run with:
//!   cargo run --example hooks-run-hook -p reloaded-code-serdesai --features mock

use reloaded_code_core::{
    EndReason, HookRunContext, HookSet, PreambleMessage, PreambleRole, RunConfig, RunHook,
    RunHookFuture, RunOriginal, RunOutput, RunUsage,
};

/// Hook that injects a system preamble before the run executes.
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
    let hooks = HookSet::builder().run_hook(PreambleInjector).build();

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
}
