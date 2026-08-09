//! Session lifecycle event types.

/// Session-compact event callback.
pub type SessionCompactFn = for<'a> fn(&'a SessionContext<'a>);

/// Session-end event callback.
pub type SessionEndFn = for<'a> fn(&'a SessionContext<'a>, EndReason);

/// Session-start event callback.
pub type SessionStartFn = for<'a> fn(&'a SessionContext<'a>);

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// Session completed normally.
    Completed,
    /// Session was stopped externally.
    Stopped,
}

/// Context given to session lifecycle events.
#[derive(Debug)]
pub struct SessionContext<'a> {
    /// Name of the agent running the session.
    pub agent_name: &'a str,
    /// Unique identifier for the current run.
    pub run_id: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_context_fields_are_accessible() {
        let ctx = SessionContext {
            agent_name: "orchestrator",
            run_id: "r3",
        };
        assert_eq!(ctx.agent_name, "orchestrator");
        assert_eq!(ctx.run_id, "r3");
    }

    #[test]
    fn end_reason_variants_exist() {
        assert_eq!(EndReason::Completed, EndReason::Completed);
        assert_eq!(EndReason::Stopped, EndReason::Stopped);
        assert_ne!(EndReason::Completed, EndReason::Stopped);
    }
}
