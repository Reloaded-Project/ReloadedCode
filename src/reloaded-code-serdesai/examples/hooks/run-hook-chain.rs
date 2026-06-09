//! Two RunHooks showing unwind order - code before original runs forward,
//! code after runs in reverse.
//!
//! Expected output:
//!   [First] before original
//!   [Second] before original
//!   [Second] after original
//!   [First] after original
//!
//! Run with:
//!   cargo run --example hooks-run-hook-chain -p reloaded-code-serdesai --features mock

use reloaded_code_core::{
    EndReason, HookRunContext, HookSet, PreambleMessage, PreambleRole, RunConfig, RunHook,
    RunHookFuture, RunOriginal, RunOutput, RunUsage,
};

/// First hook: sets system_prompt, prints before/after.
struct FirstHook;

impl RunHook for FirstHook {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        mut config: RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!("[First] before original");
            config.system_prompt = Some("FirstHook system prompt".into());
            let output = original.call(ctx, config).await?;
            println!("[First] after original");
            Ok(output)
        })
    }
}

/// Second hook: appends a preamble message, prints before/after.
struct SecondHook;

impl RunHook for SecondHook {
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        mut config: RunConfig,
        original: RunOriginal<'a>,
    ) -> RunHookFuture<'a> {
        Box::pin(async move {
            println!("[Second] before original");
            config.preamble_messages.push(PreambleMessage {
                role: PreambleRole::User,
                content: "SecondHook preamble".into(),
            });
            let output = original.call(ctx, config).await?;
            println!("[Second] after original");
            Ok(output)
        })
    }
}

#[tokio::main]
async fn main() {
    // Build the hook set with two run hooks in registration order.
    let hooks = HookSet::builder()
        .run_hook(FirstHook)
        .run_hook(SecondHook)
        .build();

    let ctx = HookRunContext {
        agent_name: "chain-demo",
        run_id: "run-001",
        model_name: "mock-model",
    };

    // Mock executor that prints the config it received.
    struct ChainExecutor;
    impl reloaded_code_core::RunExecutor for ChainExecutor {
        fn execute<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            config: RunConfig,
        ) -> RunHookFuture<'a> {
            Box::pin(async move {
                let prompt = config.system_prompt.as_deref().unwrap_or("(none)");
                let preamble_count = config.preamble_messages.len();
                println!(
                    "[Executor] system_prompt={}, preambles={}",
                    prompt, preamble_count
                );
                Ok(RunOutput {
                    content: "Chain complete.".into(),
                    reason: EndReason::Completed,
                    usage: RunUsage {
                        prompt_tokens: 200,
                        completion_tokens: 100,
                    },
                })
            })
        }
    }

    let output = hooks
        .dispatch_run(&ctx, RunConfig::default(), &ChainExecutor)
        .await
        .expect("run should succeed");

    println!(
        "[Result] content={}, reason={:?}",
        output.content, output.reason
    );
}
