//! Shared test data generators for tool benchmarks.
//!
//! Benchmark test data sourced from real Rust files (Apache-2.0, codex-rs):
//!
//!   - Small  (122 lines): core/src/tools/handlers/plan.rs
//!   - Medium (356 lines): mcp-server/src/outgoing_message.rs
//!   - Large  (770 lines): core/src/unified_exec/session_manager.rs
//!
//! # Corpus selection methodology
//!
//! These files were not picked arbitrarily. They were chosen to be
//! **statistically representative** of real Rust source code:
//!
//! 1. Every `.rs` file under `/home/sewer/Project` (~3,300 files) was scanned
//!    and three per-file metrics were recorded:
//!    - Non-blank average line length
//!    - Blank line ratio
//!    - Average bytes per line
//!
//! 2. Population-wide averages were computed across all files:
//!
//!    | Metric                       | Population average |
//!    |------------------------------|--------------------|
//!    | Non-blank avg line length    | 37.7 chars         |
//!    | Blank line ratio             | ~10.5%             |
//!    | Avg bytes/line               | 34.7               |
//!
//! 3. Each candidate file was scored by distance from these population
//!    averages. The three files above were the closest matches across the
//!    small / medium / large size brackets.

const CORPUS_SMALL_RAW: &str = include_str!("corpus_small.rs");
const CORPUS_MEDIUM_RAW: &str = include_str!("corpus_medium.rs");
const CORPUS_LARGE_RAW: &str = include_str!("corpus_large.rs");

#[derive(Clone, Copy)]
pub enum CorpusSize {
    Small,
    Medium,
    Large,
}

pub fn corpus_content(size: CorpusSize) -> &'static str {
    match size {
        CorpusSize::Small => CORPUS_SMALL_RAW,
        CorpusSize::Medium => CORPUS_MEDIUM_RAW,
        CorpusSize::Large => CORPUS_LARGE_RAW,
    }
}

#[allow(dead_code)] // Used by some benchmarks but not all
pub fn corpus_crlf(size: CorpusSize) -> String {
    corpus_content(size).replace('\n', "\r\n")
}
