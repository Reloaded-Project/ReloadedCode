//! Compact hook types: intercept trait, history entry, result, and
//! chain trampoline.
//!
//! # What compaction is
//!
//! Compaction shrinks a run's message history when its context grows
//! too large: older messages are replaced by a summary while a
//! recent window is kept verbatim.
//!
//! # The intercept point
//!
//! A compact hook wraps one compaction attempt. Code before
//! `original` sees the whole history and may rewrite it; the default
//! compaction consumes the rewritten history. Skipping `original`
//! supplies a custom [`CompactResult`] or cancels the attempt. Code
//! after `original` sees the finished outcome and unwinds in reverse
//! registration order.
//!
//! # History representation
//!
//! [`CompactMessage`] is the vendor-agnostic history entry: a
//! structured view (role and text) hooks read and rewrite, plus an
//! opaque `preserved` payload the runtime wiring fills with its
//! native history entry. The chain threads the history by value, so
//! hooks may drop, rewrite, or inject entries. Applying the history
//! back, the wiring reuses each untouched entry's `preserved`
//! original (zero loss for pass-through) and rebuilds only entries a
//! hook modified or injected, from the structured view. The mutating
//! accessors ([`CompactMessage::set_text`],
//! [`CompactMessage::set_role`]) clear `preserved`, which is how the
//! wiring tells rebuilt entries from untouched ones.
//!
//! Next: see [`HookSet::dispatch_compact`] to run the chain.
//!
//! [`HookSet::dispatch_compact`]: crate::hooks::HookSet::dispatch_compact

use crate::hooks::{HookRunContext, RunMessageRole, RunResult};
use std::any::Any;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Boxed future returned by [`CompactHook::hook`] and
/// [`CompactExecutor::execute`]. Carries the chain outcome together
/// with the history to apply.
pub type CompactHookFuture<'a> =
    Pin<Box<dyn Future<Output = RunResult<(CompactOutcome, Vec<CompactMessage>)>> + Send + 'a>>;

/// Managed trampoline to the next hook or the default compaction.
///
/// `CompactOriginal` is consumed by [`call`], so a hook continues
/// the chain at most once per attempt. There is no built-in retry: a
/// hook that wants a custom result or a cancel returns it without
/// calling the continuation.
///
/// [`call`]: Self::call
pub struct CompactOriginal<'a> {
    chain: &'a [Arc<dyn CompactHook>],
    index: usize,
    real_compact: &'a dyn CompactExecutor,
}

/// One message-history entry passed through the compact chain.
///
/// Hooks read the structured view ([`Self::role`], [`Self::text`])
/// to decide what to compact, and rewrite it through the mutating
/// accessors. Everything else the native history entry carries lives
/// in the `preserved` payload, which the runtime wiring uses to
/// apply the history back without loss.
///
/// # Remarks
///
/// `preserved` is opaque to hooks: leave it untouched. The wiring
/// reuses it verbatim for entries that pass through unchanged and
/// rebuilds the native entry from role and text once a mutation has
/// cleared it. Entries a hook injects carry no `preserved` payload.
pub struct CompactMessage {
    role: RunMessageRole,
    text: String,
    preserved: Option<Box<dyn Any + Send>>,
}

/// Outcome of one compaction attempt after the compact chain.
///
/// Callers apply the returned history on [`Self::Compacted`] and
/// leave the run's history unchanged on [`Self::Cancelled`],
/// regardless of what history the attempt returns.
#[derive(Debug, Clone, PartialEq)]
pub enum CompactOutcome {
    /// Compaction ran; the result describes it.
    Compacted(CompactResult),
    /// Compaction was cancelled; apply no history change.
    Cancelled,
}

/// Result of one compaction attempt.
///
/// The counts and names are advisory: they describe the attempt so
/// callers can report it. `tokens_before` maps onto
/// [`RunEvent::ContextCompressed`] as `original_tokens`,
/// `tokens_after` as `compressed_tokens`, and the remaining advisory
/// fields map field for field.
///
/// [`RunEvent::ContextCompressed`]: crate::hooks::RunEvent::ContextCompressed
#[derive(Debug, Clone, PartialEq)]
pub struct CompactResult {
    /// Summary text replacing the compacted messages.
    pub summary: String,
    /// Identifier of the first entry kept verbatim, when the runtime
    /// wiring has entry ids; `None` when it does not.
    pub first_kept_entry_id: Option<String>,
    /// Token count of the history before compaction.
    pub tokens_before: usize,
    /// Estimated token count of the history after compaction.
    pub tokens_after: usize,
    /// Strategy name, e.g. "summarize" or "truncate".
    pub strategy: String,
    /// Number of messages before compaction.
    pub messages_before: usize,
    /// Number of messages after compaction.
    pub messages_after: usize,
}

/// Final callable used when the compact hook chain reaches the
/// default compaction.
///
/// The default compaction summarizes older messages and returns the
/// compacted history: what it returns is the history the run
/// continues with.
pub trait CompactExecutor: Send + Sync {
    /// Runs the default compaction over `messages`.
    ///
    /// # Errors
    /// Returns [`ToolError`] when the default compaction fails.
    ///
    /// [`ToolError`]: crate::ToolError
    fn execute<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        messages: Vec<CompactMessage>,
    ) -> CompactHookFuture<'a>;
}

/// Game-style compact hook.
///
/// A hook may inspect or rewrite the history, call
/// [`CompactOriginal::call`] to continue the chain with the
/// rewritten history, inspect or adjust the finished outcome and
/// history, or skip `original` entirely to supply its own result or
/// cancel the attempt.
pub trait CompactHook: Send + Sync + 'static {
    /// Intercepts one compaction attempt.
    ///
    /// `messages` is the full history for this attempt. Mutations
    /// made before `original` are what the default compaction
    /// consumes; the returned pair carries the outcome together with
    /// the history to apply.
    ///
    /// # Errors
    /// Returns [`ToolError`] if the hook implementation or the
    /// downstream default compaction fails.
    ///
    /// [`ToolError`]: crate::ToolError
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        messages: Vec<CompactMessage>,
        original: CompactOriginal<'a>,
    ) -> CompactHookFuture<'a>;
}

impl<'a> CompactOriginal<'a> {
    /// Creates a trampoline over the provided hook chain and default
    /// compaction.
    #[inline]
    #[must_use]
    pub fn new(chain: &'a [Arc<dyn CompactHook>], real_compact: &'a dyn CompactExecutor) -> Self {
        Self {
            chain,
            index: 0,
            real_compact,
        }
    }

    /// Calls the next hook with `messages`, or the default
    /// compaction when no hooks remain.
    ///
    /// # Errors
    /// Returns [`ToolError`] if a downstream hook or the default
    /// compaction returns an error.
    ///
    /// [`ToolError`]: crate::ToolError
    #[inline]
    pub fn call(
        self,
        ctx: &'a HookRunContext<'a>,
        messages: Vec<CompactMessage>,
    ) -> CompactHookFuture<'a> {
        if let Some(hook) = self.chain.get(self.index) {
            hook.hook(
                ctx,
                messages,
                Self {
                    chain: self.chain,
                    index: self.index + 1,
                    real_compact: self.real_compact,
                },
            )
        } else {
            self.real_compact.execute(ctx, messages)
        }
    }
}

impl CompactMessage {
    /// Creates a history entry with no preserved payload.
    ///
    /// For entries a hook injects: the wiring rebuilds the native
    /// entry from role and text.
    #[inline]
    #[must_use]
    pub fn new(role: RunMessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            preserved: None,
        }
    }

    /// Creates a history entry carrying preserved native data.
    ///
    /// For the runtime wiring: `preserved` holds the native history
    /// entry this view was projected from.
    #[inline]
    #[must_use]
    pub fn new_preserved(
        role: RunMessageRole,
        text: impl Into<String>,
        preserved: Box<dyn Any + Send>,
    ) -> Self {
        Self {
            role,
            text: text.into(),
            preserved: Some(preserved),
        }
    }

    /// Returns the entry role.
    #[inline]
    #[must_use]
    pub fn role(&self) -> RunMessageRole {
        self.role
    }

    /// Returns the entry text. Empty when the entry carries no text.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the entry text and clears the preserved payload: the
    /// wiring rebuilds this entry instead of reusing the original.
    #[inline]
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.preserved = None;
    }

    /// Replaces the entry role and clears the preserved payload: the
    /// wiring rebuilds this entry instead of reusing the original.
    #[inline]
    pub fn set_role(&mut self, role: RunMessageRole) {
        self.role = role;
        self.preserved = None;
    }

    /// Returns the preserved native data, if any.
    ///
    /// Wiring layers use this to recover the original entry.
    #[inline]
    #[must_use]
    pub fn preserved(&self) -> Option<&(dyn Any + Send)> {
        self.preserved.as_deref()
    }

    /// Takes the preserved native data, leaving `None` in its place.
    ///
    /// Wiring layers use this to take ownership of the original.
    #[inline]
    pub fn take_preserved(&mut self) -> Option<Box<dyn Any + Send>> {
        self.preserved.take()
    }

    /// Returns `true` when preserved data is present: the entry
    /// passed through the chain unchanged.
    #[inline]
    #[must_use]
    pub fn has_preserved(&self) -> bool {
        self.preserved.is_some()
    }
}

impl fmt::Debug for CompactOriginal<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompactOriginal")
            .field("chain_len", &self.chain.len())
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for CompactMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompactMessage")
            .field("role", &self.role)
            .field("text", &self.text)
            .field("preserved", &self.preserved.is_some())
            .finish()
    }
}

impl<F> CompactExecutor for F
where
    F: for<'a> Fn(&'a HookRunContext<'a>, Vec<CompactMessage>) -> CompactHookFuture<'a>
        + Send
        + Sync,
{
    #[inline]
    fn execute<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        messages: Vec<CompactMessage>,
    ) -> CompactHookFuture<'a> {
        self(ctx, messages)
    }
}

impl<F> CompactHook for F
where
    F: for<'a> Fn(
            &'a HookRunContext<'a>,
            Vec<CompactMessage>,
            CompactOriginal<'a>,
        ) -> CompactHookFuture<'a>
        + Send
        + Sync
        + 'static,
{
    #[inline]
    fn hook<'a>(
        &'a self,
        ctx: &'a HookRunContext<'a>,
        messages: Vec<CompactMessage>,
        original: CompactOriginal<'a>,
    ) -> CompactHookFuture<'a> {
        self(ctx, messages, original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_ctx() -> HookRunContext<'static> {
        HookRunContext {
            agent_name: "coder",
            run_id: "r1",
            model_name: "gpt-4o",
        }
    }

    fn compact_result(summary: &str) -> CompactResult {
        CompactResult {
            summary: summary.into(),
            first_kept_entry_id: None,
            tokens_before: 100,
            tokens_after: 40,
            strategy: "summarize".into(),
            messages_before: 3,
            messages_after: 2,
        }
    }

    struct DefaultCompact;

    impl CompactExecutor for DefaultCompact {
        fn execute<'a>(
            &'a self,
            _ctx: &'a HookRunContext<'a>,
            messages: Vec<CompactMessage>,
        ) -> CompactHookFuture<'a> {
            Box::pin(async move {
                Ok((
                    CompactOutcome::Compacted(compact_result("default")),
                    messages,
                ))
            })
        }
    }

    #[test]
    fn compact_message_set_text_clears_preserved() {
        let mut message =
            CompactMessage::new_preserved(RunMessageRole::User, "old", Box::new("native"));
        assert!(message.has_preserved());
        message.set_text("rewritten");
        assert_eq!(message.text(), "rewritten");
        // A cleared payload is the wiring's signal to rebuild this
        // entry instead of reusing the original.
        assert!(!message.has_preserved());
    }

    #[test]
    fn compact_message_set_role_clears_preserved() {
        let mut message =
            CompactMessage::new_preserved(RunMessageRole::User, "old", Box::new("native"));
        message.set_role(RunMessageRole::Assistant);
        assert_eq!(message.role(), RunMessageRole::Assistant);
        assert!(!message.has_preserved());
    }

    #[test]
    fn compact_message_preserved_round_trip_returns_original() {
        // A stand-in native entry: whatever the wiring stores must
        // come back out unchanged.
        let native = String::from("native history entry");
        let mut message =
            CompactMessage::new_preserved(RunMessageRole::User, "view", Box::new(native));
        let restored = message
            .take_preserved()
            .expect("preserved entry must carry its payload");
        let restored = restored
            .downcast::<String>()
            .expect("payload type survives");
        assert_eq!(*restored, "native history entry");
        assert!(!message.has_preserved());
    }

    #[tokio::test]
    async fn compact_original_runs_executor_when_chain_empty() {
        let chain: Vec<Arc<dyn CompactHook>> = vec![];
        let (outcome, history) = CompactOriginal::new(&chain, &DefaultCompact)
            .call(&run_ctx(), Vec::new())
            .await
            .unwrap();
        assert_eq!(
            outcome,
            CompactOutcome::Compacted(compact_result("default"))
        );
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn compact_original_debug_format() {
        let chain: Vec<Arc<dyn CompactHook>> = vec![];
        let original = CompactOriginal::new(&chain, &DefaultCompact);
        let debug = format!("{original:?}");
        assert!(debug.contains("CompactOriginal"));
        assert!(debug.contains("chain_len"));
    }
}
