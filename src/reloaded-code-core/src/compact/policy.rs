//! Compaction policy: the trigger threshold and the summarize cap.

/// Default maximum output tokens requested for the summarize call.
const DEFAULT_SUMMARIZE_MAX_OUTPUT: u64 = 32_000;
/// Default trigger margin, in tokens, kept below the context limit.
const DEFAULT_TRIGGER_MARGIN: u64 = 32_000;

/// When a run should compact and how large its summarize request may
/// be.
///
/// The default triggers once a request's estimated tokens reach
/// 32,000 below the model's context limit. A context limit at or
/// below the margin cannot spare that much headroom, so the
/// proportional [`Self::trigger_fraction`] applies instead and small
/// windows still compact. The summarize request is capped at 32,000
/// output tokens.
///
/// Every field is caller-adjustable; start from [`Self::default`] and
/// override what differs:
///
/// ```
/// use reloaded_code_core::CompactPolicy;
///
/// let policy = CompactPolicy {
///     trigger_margin: 8_000,
///     ..CompactPolicy::default()
/// };
/// assert_eq!(policy.trigger_threshold(100_000), Some(92_000));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactPolicy {
    /// Headroom, in tokens, between the trigger threshold and the
    /// context limit.
    pub trigger_margin: u64,
    /// Proportional threshold for context limits at or below
    /// [`Self::trigger_margin`].
    pub trigger_fraction: CompactFraction,
    /// Maximum output tokens requested for the summarize call.
    pub summarize_max_output: u64,
}

/// Proportional share used for small-window trigger thresholds.
///
/// Held as an integer fraction so threshold math stays exact and
/// allocation-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactFraction {
    numerator: u64,
    denominator: u64,
}

impl CompactPolicy {
    /// Token threshold that triggers compaction for
    /// `context_limit`, or `None` while the limit is zero and
    /// therefore unusable.
    ///
    /// Limits above the margin trigger at `limit - margin`; limits
    /// at or below it trigger at the proportional fallback.
    #[inline]
    #[must_use]
    pub fn trigger_threshold(&self, context_limit: u64) -> Option<u64> {
        match context_limit {
            0 => None,
            limit if limit > self.trigger_margin => Some(limit - self.trigger_margin),
            limit => Some(self.trigger_fraction.apply(limit)),
        }
    }

    /// Returns `true` when a request estimating `estimated_tokens`
    /// tokens should compact under `context_limit`.
    ///
    /// Compaction triggers once the estimate reaches the threshold
    /// [`Self::trigger_threshold`] returns. An unknown (`None`) or
    /// zero limit never triggers.
    #[inline]
    #[must_use]
    pub fn should_compact(&self, context_limit: Option<u64>, estimated_tokens: u64) -> bool {
        context_limit
            .and_then(|limit| self.trigger_threshold(limit))
            .is_some_and(|threshold| estimated_tokens >= threshold)
    }

    /// Maximum output tokens for the summarize request.
    ///
    /// The policy cap clamped to the model's advertised maximum
    /// output when `max_output` is known.
    #[inline]
    #[must_use]
    pub fn summarize_cap(&self, max_output: Option<u64>) -> u64 {
        self.summarize_max_output
            .min(max_output.unwrap_or(u64::MAX))
    }
}

impl CompactFraction {
    /// Three quarters; the default small-window fallback.
    pub const THREE_QUARTERS: Self = Self {
        numerator: 3,
        denominator: 4,
    };

    /// Creates the fraction `numerator / denominator`.
    ///
    /// # Panics
    /// Panics when `denominator` is zero.
    #[must_use]
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        assert!(denominator != 0, "fraction denominator must be non-zero");
        Self {
            numerator,
            denominator,
        }
    }

    /// Returns `value * numerator / denominator`, saturating on
    /// overflow.
    fn apply(self, value: u64) -> u64 {
        value.saturating_mul(self.numerator) / self.denominator
    }
}

impl Default for CompactPolicy {
    /// Margin 32,000, fraction 3/4, summarize cap 32,000.
    fn default() -> Self {
        Self {
            trigger_margin: DEFAULT_TRIGGER_MARGIN,
            trigger_fraction: CompactFraction::THREE_QUARTERS,
            summarize_max_output: DEFAULT_SUMMARIZE_MAX_OUTPUT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_threshold_maps_small_windows_to_three_quarters() {
        let policy = CompactPolicy::default();
        assert_eq!(policy.trigger_threshold(0), None);
        assert_eq!(policy.trigger_threshold(8), Some(6));
        assert_eq!(policy.trigger_threshold(1_024), Some(768));
        // The margin boundary itself counts as a small window.
        assert_eq!(policy.trigger_threshold(32_000), Some(24_000));
    }

    #[test]
    fn trigger_threshold_subtracts_the_margin_above_it() {
        let policy = CompactPolicy::default();
        assert_eq!(policy.trigger_threshold(32_001), Some(1));
        assert_eq!(policy.trigger_threshold(200_000), Some(168_000));
    }

    #[test]
    fn trigger_margin_override_moves_the_threshold() {
        let policy = CompactPolicy {
            trigger_margin: 8_000,
            ..CompactPolicy::default()
        };
        assert_eq!(policy.trigger_threshold(100_000), Some(92_000));
        // The override also moves the fallback boundary.
        assert_eq!(policy.trigger_threshold(8_000), Some(6_000));
    }

    #[test]
    fn trigger_fraction_override_moves_small_window_thresholds() {
        let policy = CompactPolicy {
            trigger_fraction: CompactFraction::new(1, 2),
            ..CompactPolicy::default()
        };
        assert_eq!(policy.trigger_threshold(1_000), Some(500));
        // Large windows keep the margin behavior.
        assert_eq!(policy.trigger_threshold(200_000), Some(168_000));
    }

    #[test]
    fn should_compact_triggers_only_at_or_past_the_threshold() {
        let policy = CompactPolicy::default();
        let limit = 200_000;
        let threshold = 168_000;
        assert!(!policy.should_compact(None, u64::MAX), "unknown limit");
        assert!(!policy.should_compact(Some(0), u64::MAX), "zero limit");
        assert!(!policy.should_compact(Some(limit), threshold - 1));
        assert!(policy.should_compact(Some(limit), threshold));
        assert!(policy.should_compact(Some(limit), threshold + 1));
    }

    #[test]
    fn summarize_cap_defaults_to_32k_and_clamps_to_the_model_limit() {
        let policy = CompactPolicy::default();
        assert_eq!(policy.summarize_cap(None), 32_000);
        assert_eq!(policy.summarize_cap(Some(100_000)), 32_000);
        assert_eq!(policy.summarize_cap(Some(8_000)), 8_000);
    }

    #[test]
    fn summarize_cap_override_wins_up_to_the_model_limit() {
        let policy = CompactPolicy {
            summarize_max_output: 16_000,
            ..CompactPolicy::default()
        };
        assert_eq!(policy.summarize_cap(None), 16_000);
        assert_eq!(policy.summarize_cap(Some(4_000)), 4_000);
    }

    #[test]
    fn fraction_new_builds_custom_fractions() {
        assert_eq!(CompactFraction::new(3, 4), CompactFraction::THREE_QUARTERS);
    }

    #[test]
    #[should_panic(expected = "fraction denominator must be non-zero")]
    fn fraction_new_panics_on_zero_denominator() {
        let _ = CompactFraction::new(1, 0);
    }
}
