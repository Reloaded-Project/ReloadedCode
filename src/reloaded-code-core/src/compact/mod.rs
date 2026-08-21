//! Vendor-free context compaction: policy, planning, and the
//! summarize port.
//!
//! # Public API
//!
//! Policy:
//! - [`CompactPolicy`] - When to compact and how large the summarize
//!   request may be
//! - [`CompactFraction`] - Proportional trigger threshold for small
//!   context windows
//!
//! Planning:
//! - [`CompactEntry`] - Neutral transcript entry carried through
//!   planning
//! - [`Compactor`] - Plans and applies compactions over neutral
//!   entries
//! - [`Compaction`] - Applied compaction: compacted history plus
//!   [`CompactionRecord`]
//!
//! Port:
//! - [`SummaryExecutor`] - Runs the summarize request on the run's
//!   model
//! - [`SummaryRequest`] - One summarize request handed through the
//!   port
//!
//! # Design
//!
//! Decision and planning are pure in-crate computation; the only
//! boundary is the [`SummaryExecutor`] port.
//!
//! Inputs:
//! - Context limit for the run's model
//! - Token estimate for the pending request
//! - Transcript of [`CompactEntry`] values
//!
//! Outputs:
//! - Decision to compact
//! - Compacted transcript
//! - [`CompactionRecord`] describing the applied compaction
//!
//! Runtime wiring implements the port over the run's model, maps
//! vendor histories onto [`CompactEntry`], and reports applied
//! compactions as events.
//!
//! Next: see [`CompactPolicy`] for the trigger and cap defaults.
pub use entry::CompactEntry;
pub use planner::{Compaction, CompactionRecord, Compactor};
pub use policy::{CompactFraction, CompactPolicy};
pub use port::{SummaryExecutor, SummaryFuture, SummaryRequest};

mod entry;
mod planner;
mod policy;
mod port;
