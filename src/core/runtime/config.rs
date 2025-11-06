//! Immutable runtime configuration shared across threads when `sync` is enabled.
use crate::core::reaction::scheduler::{InlineScheduler, ReactionScheduler};
use std::sync::Arc;

/// Static configuration values for the MobX runtime.
#[derive(Clone)]
pub(crate) struct RuntimeConfig {
    reaction_scheduler: Arc<dyn ReactionScheduler>,
}

impl RuntimeConfig {
    pub(crate) fn reaction_scheduler(&self) -> Arc<dyn ReactionScheduler> {
        Arc::clone(&self.reaction_scheduler)
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            reaction_scheduler: Arc::new(InlineScheduler::default()),
        }
    }
}
