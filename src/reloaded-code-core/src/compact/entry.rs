//! Neutral transcript entry carried through compaction planning.

use crate::hooks::RunMessageRole;
use std::any::Any;
use std::fmt;

/// One message-history entry passed through compaction planning.
///
/// An entry has two parts:
/// - Structured view ([`Self::role`], [`Self::text`]): what planning
///   reads and what a rebuilt history is built from.
/// - Preserved payload: everything else the native history entry
///   carries. The runtime wiring fills it in and reuses it so an
///   untouched entry survives compaction without loss.
///
/// # Remarks
///
/// The payload is opaque to this crate: leave it untouched. Entries
/// built by [`Self::new`] carry no payload.
pub struct CompactEntry {
    role: RunMessageRole,
    text: String,
    /// Preserved payload: the type-erased native history entry the
    /// runtime wiring reuses when the entry survives compaction.
    preserved: Option<Box<dyn Any + Send>>,
}

impl CompactEntry {
    /// Creates a payload-free entry from its structured view.
    #[must_use]
    pub fn new(role: RunMessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
            preserved: None,
        }
    }

    /// Creates an entry preserving `preserved` as its native payload.
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

    /// Author role of the entry.
    #[must_use]
    pub fn role(&self) -> RunMessageRole {
        self.role
    }

    /// Text of the entry as planning and the summarizer see it.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns `true` while the native payload is intact.
    #[must_use]
    pub fn has_preserved(&self) -> bool {
        self.preserved.is_some()
    }

    /// Takes the native payload, leaving the entry payload-free.
    #[must_use]
    pub fn take_preserved(&mut self) -> Option<Box<dyn Any + Send>> {
        self.preserved.take()
    }
}

impl fmt::Debug for CompactEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The payload is opaque, so only its presence is reported.
        f.debug_struct("CompactEntry")
            .field("role", &self.role)
            .field("text", &self.text)
            .field("preserved", &self.preserved.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_preserved_returns_the_native_payload_once() {
        let mut entry = CompactEntry::new_preserved(
            RunMessageRole::User,
            "question",
            Box::new(String::from("native part")),
        );
        assert!(entry.has_preserved());

        let preserved = entry
            .take_preserved()
            .expect("the payload is present on the first take");
        let part = preserved
            .downcast::<String>()
            .expect("the payload keeps its type");
        assert_eq!(*part, "native part");

        assert!(!entry.has_preserved());
        assert!(
            entry.take_preserved().is_none(),
            "the payload is taken once"
        );
        // The structured view survives the take.
        assert_eq!(entry.role(), RunMessageRole::User);
        assert_eq!(entry.text(), "question");
    }

    #[test]
    fn new_builds_a_payload_free_entry() {
        let entry = CompactEntry::new(RunMessageRole::System, "sys");
        assert_eq!(entry.role(), RunMessageRole::System);
        assert_eq!(entry.text(), "sys");
        assert!(!entry.has_preserved());
    }
}
