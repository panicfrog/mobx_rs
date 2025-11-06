#![allow(dead_code)] // Phase 2 scaffolding exposes APIs for later derivation work.

//! Observable primitives that underpin MobX-style dependency tracking.
//!
//! Atoms form the minimal observable units. They track their observers, expose
//! lifecycle hooks for when they become observed or unobserved, and integrate
//! with the runtime's batch and state read/write policies.

use crate::core::derivation::{self, DerivationState};
use crate::core::runtime;
use crate::core::spy::{self, SpyEvent};
use crate::internal::ids::{DerivationId, ObservableId};
use crate::internal::registry::IdRegistry;
use crate::internal::shared::{Shared, SharedCell, SharedReadGuard, new_shared};
#[cfg(feature = "sync")]
use parking_lot::Mutex;
#[cfg(not(feature = "sync"))]
use std::cell::RefCell;
#[cfg(feature = "sync")]
use std::sync::OnceLock;

#[cfg(feature = "sync")]
pub(crate) type DynObservableCore = (dyn ObservableCore + Send + Sync);
#[cfg(not(feature = "sync"))]
pub(crate) type DynObservableCore = dyn ObservableCore;

#[cfg(feature = "sync")]
type AtomCallback = Box<dyn FnMut() + Send + Sync>;
#[cfg(not(feature = "sync"))]
type AtomCallback = Box<dyn FnMut()>;

#[cfg(feature = "sync")]
fn observable_registry() -> &'static Mutex<IdRegistry<ObservableId, DynObservableCore>> {
    static REGISTRY: OnceLock<Mutex<IdRegistry<ObservableId, DynObservableCore>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(IdRegistry::new()))
}

#[cfg(not(feature = "sync"))]
thread_local! {
    static OBSERVABLE_REGISTRY: RefCell<IdRegistry<ObservableId, DynObservableCore>> =
        RefCell::new(IdRegistry::new());
}

pub(crate) fn reserve_observable_id() -> ObservableId {
    #[cfg(feature = "sync")]
    {
        let mut registry = observable_registry().lock();
        return registry.reserve();
    }

    #[cfg(not(feature = "sync"))]
    {
        return OBSERVABLE_REGISTRY.with(|registry| registry.borrow_mut().reserve());
    }
}

pub(crate) fn attach_observable(id: ObservableId, observable: &Shared<DynObservableCore>) {
    #[cfg(feature = "sync")]
    {
        observable_registry().lock().attach(id, observable);
    }

    #[cfg(not(feature = "sync"))]
    {
        OBSERVABLE_REGISTRY.with(|registry| registry.borrow_mut().attach(id, observable));
    }
}

pub(crate) fn with_observable<R>(
    id: ObservableId,
    f: impl FnOnce(&dyn ObservableCore) -> R,
) -> Option<R> {
    #[cfg(feature = "sync")]
    {
        let observable = {
            let registry = observable_registry().lock();
            registry.get(id)
        };
        observable.map(|observable| f(observable.as_ref()))
    }

    #[cfg(not(feature = "sync"))]
    {
        OBSERVABLE_REGISTRY
            .with(|registry| registry.borrow().get(id))
            .map(|observable| f(observable.as_ref()))
    }
}

/// Core behavior required from any observable type in the runtime.
pub(crate) trait ObservableCore {
    /// Unique identifier for the observable.
    fn id(&self) -> ObservableId;

    /// Human-readable name for debugging and diagnostics.
    fn name(&self) -> &str;

    /// Reports that the observable has been read while tracking dependencies.
    fn report_observed(&self) -> bool;

    /// Registers the provided derivation as an observer.
    fn add_observer(&self, derivation: DerivationId);

    /// Removes the observer if it was previously registered.
    fn remove_observer(&self, derivation: DerivationId);

    /// Reports that the observable's value has changed.
    fn report_changed(&self);

    /// Invokes the `on_become_observed` lifecycle hook if set.
    fn on_become_observed(&self);

    /// Invokes the `on_become_unobserved` lifecycle hook if set.
    fn on_become_unobserved(&self);

    /// Returns an immutable view into the observer list.
    fn observers(&self) -> SharedReadGuard<'_, Vec<DerivationId>>;

    /// Current diff value used for structural comparison.
    fn diff_value(&self) -> u8;

    /// Sets the diff value.
    fn set_diff_value(&self, value: u8);

    /// Indicates whether the observable is actively observed.
    fn is_being_observed(&self) -> bool;

    /// Updates the tracked observation flag.
    fn set_being_observed(&self, value: bool);

    /// Returns whether the observable is queued for unobservation.
    fn is_pending_unobservation(&self) -> bool;

    /// Marks the observable as pending unobservation.
    fn set_pending_unobservation(&self, value: bool);

    /// Returns the lowest observer state captured during propagation.
    fn lowest_observer_state(&self) -> DerivationState;

    /// Sets the lowest observer state.
    fn set_lowest_observer_state(&self, state: DerivationState);
}

/// Minimal observable entity used to model atomic state.
pub(crate) struct Atom {
    id: ObservableId,
    name: String,
    observers: SharedCell<Vec<DerivationId>>,
    diff_value: SharedCell<u8>,
    is_being_observed: SharedCell<bool>,
    is_pending_unobservation: SharedCell<bool>,
    lowest_observer_state: SharedCell<DerivationState>,
    on_become_observed: SharedCell<Option<AtomCallback>>,
    on_become_unobserved: SharedCell<Option<AtomCallback>>,
}

impl Atom {
    /// Creates a new atom with optional lifecycle hooks.
    pub(crate) fn new(
        name: impl Into<String>,
        on_become_observed: Option<AtomCallback>,
        on_become_unobserved: Option<AtomCallback>,
    ) -> Shared<Self> {
        let id = reserve_observable_id();
        let atom = new_shared(Self {
            id,
            name: name.into(),
            observers: SharedCell::new(Vec::new()),
            diff_value: SharedCell::new(0),
            is_being_observed: SharedCell::new(false),
            is_pending_unobservation: SharedCell::new(false),
            lowest_observer_state: SharedCell::new(DerivationState::UpToDate),
            on_become_observed: SharedCell::new(on_become_observed),
            on_become_unobserved: SharedCell::new(on_become_unobserved),
        });

        let trait_obj: Shared<DynObservableCore> = atom.clone();
        attach_observable(id, &trait_obj);
        atom
    }

    /// Adds an observer to the atom's dependency list.
    pub(crate) fn attach_observer(&self, derivation: DerivationId) {
        let mut observers = self.observers.borrow_mut();
        if !observers.contains(&derivation) {
            observers.push(derivation);
        }
        self.set_being_observed(true);
        self.set_pending_unobservation(false);
    }

    /// Removes the specified observer if present.
    pub(crate) fn detach_observer(&self, derivation: DerivationId) {
        let mut observers = self.observers.borrow_mut();
        if let Some(index) = observers.iter().position(|id| *id == derivation) {
            observers.swap_remove(index);
        }
        if observers.is_empty() {
            self.set_pending_unobservation(true);
            self.set_being_observed(false);
            self.invoke_on_become_unobserved();
        }
    }

    fn invoke_on_become_observed(&self) {
        let mut guard = self.on_become_observed.borrow_mut();
        if let Some(callback) = guard.as_mut() {
            callback();
        }
    }

    fn invoke_on_become_unobserved(&self) {
        let mut guard = self.on_become_unobserved.borrow_mut();
        if let Some(callback) = guard.as_mut() {
            callback();
        }
    }
}

impl ObservableCore for Atom {
    fn id(&self) -> ObservableId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn report_observed(&self) -> bool {
        if !runtime::state_reads_allowed() {
            return false;
        }

        if spy::is_enabled() {
            spy::report(SpyEvent::ObservableRead {
                name: self.name.clone(),
            });
        }

        if let Some(derivation) =
            runtime::current_tracking_derivation().and_then(|weak| weak.upgrade())
        {
            let mut new_observing = derivation.new_observing();
            new_observing.push(self.id);
            derivation.set_unbound_deps_count(new_observing.len());
        }

        if !self.is_being_observed() {
            self.set_being_observed(true);
            self.set_pending_unobservation(false);
            self.invoke_on_become_observed();
        }

        true
    }

    fn add_observer(&self, derivation: DerivationId) {
        self.attach_observer(derivation);
    }

    fn remove_observer(&self, derivation: DerivationId) {
        self.detach_observer(derivation);
    }

    fn report_changed(&self) {
        if let Some(warning) =
            runtime::record_strict_mode_violation(self.name(), self.is_being_observed())
        {
            if spy::is_enabled() {
                spy::report(SpyEvent::StrictModeViolation {
                    message: warning.clone(),
                });
            }
            eprintln!("{warning}");
        }

        if spy::is_enabled() {
            spy::report(SpyEvent::ObservableWrite {
                name: self.name.clone(),
            });
        }

        let _batch = runtime::start_batch();
        self.set_lowest_observer_state(DerivationState::Stale);
        self.set_pending_unobservation(false);
        let next = self.diff_value().wrapping_add(1);
        self.set_diff_value(next);

        let observers: Vec<DerivationId> = self.observers.borrow().clone();
        for derivation_id in observers {
            let _ = derivation::with_derivation(derivation_id, |derivation| {
                match derivation.dependencies_state() {
                    DerivationState::UpToDate | DerivationState::PossiblyStale => {
                        derivation.set_dependencies_state(DerivationState::Stale);
                        derivation.on_become_stale();
                    }
                    DerivationState::NotTracking | DerivationState::Stale => {}
                }
            });
        }
    }

    fn on_become_observed(&self) {
        self.invoke_on_become_observed();
    }

    fn on_become_unobserved(&self) {
        self.invoke_on_become_unobserved();
    }

    fn observers(&self) -> SharedReadGuard<'_, Vec<DerivationId>> {
        self.observers.borrow()
    }

    fn diff_value(&self) -> u8 {
        *self.diff_value.borrow()
    }

    fn set_diff_value(&self, value: u8) {
        *self.diff_value.borrow_mut() = value;
    }

    fn is_being_observed(&self) -> bool {
        *self.is_being_observed.borrow()
    }

    fn set_being_observed(&self, value: bool) {
        *self.is_being_observed.borrow_mut() = value;
    }

    fn is_pending_unobservation(&self) -> bool {
        *self.is_pending_unobservation.borrow()
    }

    fn set_pending_unobservation(&self, value: bool) {
        *self.is_pending_unobservation.borrow_mut() = value;
    }

    fn lowest_observer_state(&self) -> DerivationState {
        *self.lowest_observer_state.borrow()
    }

    fn set_lowest_observer_state(&self, state: DerivationState) {
        *self.lowest_observer_state.borrow_mut() = state;
    }
}

#[cfg(all(test, not(feature = "sync")))]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::num::NonZeroU64;
    use std::rc::Rc;

    fn make_counter() -> Rc<Cell<usize>> {
        Rc::new(Cell::new(0))
    }

    fn bump(counter: &Rc<Cell<usize>>) {
        counter.set(counter.get() + 1);
    }

    #[test]
    fn test_report_observed_triggers_callback_once() {
        let observed_calls = make_counter();
        let atom = Atom::new(
            "atom",
            Some(Box::new({
                let observed_calls = Rc::clone(&observed_calls);
                move || bump(&observed_calls)
            })),
            None,
        );

        assert_eq!(observed_calls.get(), 0);
        assert!(atom.report_observed());
        assert!(atom.is_being_observed());
        assert_eq!(observed_calls.get(), 1);

        // Subsequent observations should not retrigger the hook.
        assert!(atom.report_observed());
        assert_eq!(observed_calls.get(), 1);
    }

    #[test]
    fn test_add_and_remove_observer_updates_state() {
        let observed_calls = make_counter();
        let unobserved_calls = make_counter();
        let atom = Atom::new(
            "atom",
            Some(Box::new({
                let observed_calls = Rc::clone(&observed_calls);
                move || bump(&observed_calls)
            })),
            Some(Box::new({
                let unobserved_calls = Rc::clone(&unobserved_calls);
                move || bump(&unobserved_calls)
            })),
        );

        let derivation_id = DerivationId::from(NonZeroU64::new(1).unwrap());

        atom.report_observed();
        atom.attach_observer(derivation_id);
        assert!(atom.is_being_observed());

        atom.attach_observer(derivation_id);
        assert_eq!(atom.observers().len(), 1);

        atom.detach_observer(derivation_id);
        assert!(!atom.is_being_observed());
        assert!(atom.is_pending_unobservation());
        assert_eq!(unobserved_calls.get(), 1);
        assert_eq!(observed_calls.get(), 1);
    }

    #[test]
    fn test_report_changed_updates_diff_and_state() {
        let atom = Atom::new("atom", None, None);
        let before = atom.diff_value();
        atom.report_changed();
        assert_eq!(atom.diff_value(), before.wrapping_add(1));
        assert_eq!(atom.lowest_observer_state(), DerivationState::Stale);
    }

    #[test]
    fn test_observable_ids_are_unique() {
        let a = Atom::new("atom_a", None, None);
        let b = Atom::new("atom_b", None, None);
        assert_ne!(a.id(), b.id());
    }
}
