//! Pluggable reaction scheduling strategies.
//!
//! The default inline scheduler flushes pending reactions immediately on the
//! current thread. Alternative schedulers can enqueue work to background
//! executors or event loops to integrate with external runtimes.

use super::run_pending_reactions;

/// Dispatches pending reactions when observers schedule work.
pub(crate) trait ReactionScheduler: Send + Sync {
    /// Requests that pending reactions be processed.
    fn schedule(&self);
}

/// Inline scheduler that immediately drains the reaction queue on the current thread.
#[derive(Default)]
pub(crate) struct InlineScheduler;

impl ReactionScheduler for InlineScheduler {
    fn schedule(&self) {
        run_pending_reactions();
    }
}
