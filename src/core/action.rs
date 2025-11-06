//! Action utilities that mirror MobX's state mutation windows.

use crate::core::runtime::{
    self, AllowStateChangesGuard, AllowStateReadsGuard, BatchGuard, EnforcePolicy,
};
use crate::core::spy::{self, SpyEvent};

/// Policies that determine when state modifications must occur inside actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionPolicy {
    /// Never enforce actions – state changes are always permitted.
    Never,
    /// Only enforce actions for observables that are currently observed.
    Observed,
    /// Always enforce actions – all mutations must happen within an action.
    Always,
}

impl From<ActionPolicy> for EnforcePolicy {
    fn from(policy: ActionPolicy) -> Self {
        match policy {
            ActionPolicy::Never => EnforcePolicy::Never,
            ActionPolicy::Observed => EnforcePolicy::Observed,
            ActionPolicy::Always => EnforcePolicy::Always,
        }
    }
}

impl From<EnforcePolicy> for ActionPolicy {
    fn from(policy: EnforcePolicy) -> Self {
        match policy {
            EnforcePolicy::Never => ActionPolicy::Never,
            EnforcePolicy::Observed => ActionPolicy::Observed,
            EnforcePolicy::Always => ActionPolicy::Always,
        }
    }
}

struct ActionContext {
    name: String,
    emit_spy_events: bool,
    _allow_state_reads_guard: AllowStateReadsGuard,
    _allow_state_changes_guard: AllowStateChangesGuard,
    _batch_guard: BatchGuard,
}

impl ActionContext {
    fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let emit_spy_events = spy::is_enabled();
        if emit_spy_events {
            spy::report(SpyEvent::ActionStart { name: name.clone() });
        }

        let batch_guard = runtime::start_batch();
        let allow_state_reads_guard = runtime::allow_state_reads_guard(true);
        let allow_state_changes_guard = runtime::allow_state_changes_guard(true);

        Self {
            name,
            emit_spy_events,
            _allow_state_reads_guard: allow_state_reads_guard,
            _allow_state_changes_guard: allow_state_changes_guard,
            _batch_guard: batch_guard,
        }
    }
}

impl Drop for ActionContext {
    fn drop(&mut self) {
        if self.emit_spy_events {
            spy::report(SpyEvent::ActionEnd {
                name: self.name.clone(),
            });
        }
    }
}

/// Runs the provided closure inside an action with the given name.
pub fn action<T>(name: impl Into<String>, f: impl FnOnce() -> T) -> T {
    let ctx = ActionContext::new(name);
    let result = f();
    drop(ctx);
    result
}

/// Convenience helper that mirrors MobX's `runInAction`.
pub fn run_in_action<T>(f: impl FnOnce() -> T) -> T {
    action("run_in_action", f)
}

/// Temporarily allows state changes regardless of the current enforcement policy.
pub fn allow_state_changes<T>(f: impl FnOnce() -> T) -> T {
    let _guard = runtime::allow_state_changes_guard(true);
    f()
}

/// Configures the global action enforcement policy.
pub fn set_enforce_actions(policy: ActionPolicy) {
    runtime::set_enforce_policy(policy.into());
}

/// Returns the currently configured enforcement policy.
pub fn enforce_actions_policy() -> ActionPolicy {
    runtime::enforce_policy().into()
}

/// Drains and returns any strict-mode warnings produced since the last call.
pub fn drain_strict_mode_warnings() -> Vec<String> {
    runtime::drain_strict_mode_warnings()
}

#[cfg(all(test, not(feature = "sync")))]
mod tests {
    use super::*;
    use crate::core::observable::{Atom, ObservableCore};
    use crate::core::runtime;

    #[test]
    fn test_action_allows_state_changes_and_restores_policy() {
        runtime::with_runtime(|runtime| runtime.reset());
        set_enforce_actions(ActionPolicy::Always);

        let atom = Atom::new("action_atom", None, None);
        run_in_action(|| atom.report_changed());
        assert!(drain_strict_mode_warnings().is_empty());

        atom.report_changed();
        let warnings = drain_strict_mode_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("action_atom"),
            "expected warning to reference atom name"
        );

        set_enforce_actions(ActionPolicy::Never);
    }
}
