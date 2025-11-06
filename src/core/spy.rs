//! Spy subsystem for observing runtime events.

use crate::core::runtime;
use crate::internal::shared::{Shared, SharedCell, new_shared};

/// Event emitted by the runtime for debugging or tooling purposes.
#[derive(Clone, Debug, PartialEq)]
pub enum SpyEvent {
    ActionStart {
        name: String,
    },
    ActionEnd {
        name: String,
    },
    ObservableRead {
        name: String,
    },
    ObservableWrite {
        name: String,
    },
    ReactionScheduled {
        name: String,
        reaction_id: u64,
    },
    ReactionRunStart {
        name: String,
        reaction_id: u64,
    },
    ReactionRunEnd {
        name: String,
        reaction_id: u64,
    },
    ReactionDisposed {
        name: String,
        reaction_id: u64,
    },
    ComputedStart {
        name: String,
        derivation_id: u64,
    },
    ComputedEnd {
        name: String,
        derivation_id: u64,
        changed: bool,
    },
    StrictModeViolation {
        message: String,
    },
}

#[cfg(feature = "sync")]
type SpyCallback = dyn FnMut(&SpyEvent) + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type SpyCallback = dyn FnMut(&SpyEvent) + 'static;

pub(crate) type SpyListener = Shared<SharedCell<Box<SpyCallback>>>;

/// Subscription handle returned from [`register`]. Dropping the subscription
/// automatically unregisters the listener.
pub struct SpySubscription {
    id: u64,
    disposed: bool,
}

impl SpySubscription {
    /// Manually disposes the subscription, removing the listener from the registry.
    pub fn dispose(&mut self) {
        if !self.disposed {
            runtime::remove_spy_listener(self.id);
            self.disposed = true;
        }
    }

    /// Returns whether the subscription has been disposed.
    pub fn is_disposed(&self) -> bool {
        self.disposed
    }
}

impl Drop for SpySubscription {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Registers a new spy listener. Returns a subscription handle that can be
/// disposed to stop receiving events.
#[cfg(feature = "sync")]
pub fn register(listener: impl FnMut(&SpyEvent) + Send + Sync + 'static) -> SpySubscription {
    let listener: SpyListener = new_shared(SharedCell::new(Box::new(listener)));
    let id = runtime::add_spy_listener(listener);
    SpySubscription {
        id,
        disposed: false,
    }
}

/// Registers a new spy listener. Returns a subscription handle that can be
/// disposed to stop receiving events.
#[cfg(not(feature = "sync"))]
pub fn register(listener: impl FnMut(&SpyEvent) + 'static) -> SpySubscription {
    let listener: SpyListener = new_shared(SharedCell::new(Box::new(listener)));
    let id = runtime::add_spy_listener(listener);
    SpySubscription {
        id,
        disposed: false,
    }
}

/// Returns whether any spy listeners are currently registered.
pub fn is_enabled() -> bool {
    runtime::has_spy_listeners()
}

/// Emits an event to all registered listeners.
pub(crate) fn report(event: SpyEvent) {
    runtime::emit_spy_event(event);
}

#[cfg(all(test, not(feature = "sync")))]
mod tests {
    use super::*;
    use crate::core::action::{
        ActionPolicy, action, drain_strict_mode_warnings, set_enforce_actions,
    };
    use crate::core::observable::{Atom, ObservableCore};
    use crate::core::reaction::{autorun, run_pending_reactions};
    use crate::core::runtime;
    use crate::internal::shared::{SharedCell, new_shared};

    #[test]
    fn test_spy_receives_action_and_observable_events() {
        runtime::with_runtime(|runtime| runtime.reset());
        drain_strict_mode_warnings();

        let captured = new_shared(SharedCell::new(Vec::new()));
        let captured_clone = captured.clone();
        let mut subscription = register(move |event| {
            captured_clone.borrow_mut().push(event.clone());
        });

        let atom = Atom::new("spy_atom", None, None);
        action("spy_action", || {
            atom.report_observed();
            atom.report_changed();
        });

        subscription.dispose();

        let events = captured.borrow();
        assert!(events.iter().any(|event| {
            matches!(event, SpyEvent::ActionStart { name } if name == "spy_action")
        }));
        assert!(events.iter().any(|event| {
            matches!(event, SpyEvent::ActionEnd { name } if name == "spy_action")
        }));
        assert!(events.iter().any(|event| {
            matches!(event, SpyEvent::ObservableRead { name } if name == "spy_atom")
        }));
        assert!(events.iter().any(|event| {
            matches!(event, SpyEvent::ObservableWrite { name } if name == "spy_atom")
        }));
    }

    #[test]
    fn test_spy_tracks_reaction_lifecycle() {
        runtime::with_runtime(|runtime| runtime.reset());
        let captured = new_shared(SharedCell::new(Vec::new()));
        let captured_clone = captured.clone();
        let mut subscription = register(move |event| {
            captured_clone.borrow_mut().push(event.clone());
        });

        let atom = Atom::new("reaction_atom", None, None);
        let counter = new_shared(SharedCell::new(0));
        let counter_clone = counter.clone();
        let atom_for_reaction = atom.clone();

        let handle = autorun(move || {
            atom_for_reaction.report_observed();
            let mut value = counter_clone.borrow_mut();
            *value += 1;
        });

        atom.report_changed();
        run_pending_reactions();
        handle.dispose();
        subscription.dispose();

        let events = captured.borrow();
        assert!(matches!(
            events.first(),
            Some(SpyEvent::ReactionRunStart { .. })
        ));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SpyEvent::ReactionScheduled { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SpyEvent::ReactionRunEnd { .. }))
        );
        assert!(events.iter().any(|event| {
            matches!(event, SpyEvent::ObservableWrite { name } if name == "reaction_atom")
        }));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SpyEvent::ObservableRead { .. }))
        );
    }

    #[test]
    fn test_strict_mode_violation_emits_event() {
        runtime::with_runtime(|runtime| runtime.reset());
        let captured = new_shared(SharedCell::new(Vec::new()));
        let captured_clone = captured.clone();
        let mut subscription = register(move |event| {
            captured_clone.borrow_mut().push(event.clone());
        });

        set_enforce_actions(ActionPolicy::Always);
        drain_strict_mode_warnings();

        let atom = Atom::new("strict_atom", None, None);
        atom.report_changed();

        subscription.dispose();
        set_enforce_actions(ActionPolicy::Never);

        let events = captured.borrow();
        assert!(events.iter().any(|event| matches!(event, SpyEvent::StrictModeViolation { message } if message.contains("strict_atom"))));
    }
}
