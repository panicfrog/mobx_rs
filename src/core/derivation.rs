#![allow(dead_code)] // Derivation state variants will be consumed in later phases.

//! Shared derivation types used across computed values and reactions.

use crate::core::observable;
use crate::core::runtime;
use crate::internal::ids::{DerivationId, ObservableId};
use crate::internal::registry::IdRegistry;
use crate::internal::shared::{self, Shared, SharedReadGuard, SharedWriteGuard};
#[cfg(feature = "sync")]
use parking_lot::Mutex;
#[cfg(not(feature = "sync"))]
use std::cell::RefCell;
use std::collections::HashSet;
#[cfg(feature = "sync")]
use std::sync::OnceLock;

#[cfg(feature = "sync")]
type DynDerivation = (dyn Derivation + Send + Sync);
#[cfg(not(feature = "sync"))]
type DynDerivation = dyn Derivation;

pub(crate) type DerivationPtr = Shared<DynDerivation>;
pub(crate) type DerivationWeak = crate::internal::shared::SharedWeak<DynDerivation>;

/// Represents the freshness status of a derivation relative to its dependencies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DerivationState {
    NotTracking,
    UpToDate,
    PossiblyStale,
    Stale,
}

impl Default for DerivationState {
    fn default() -> Self {
        Self::NotTracking
    }
}

/// Controls how much tracing information should be emitted for a derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TraceMode {
    None,
    Log,
    Break,
}

/// Core behaviour shared between reactions and computed values.
pub(crate) trait Derivation {
    fn id(&self) -> DerivationId;
    fn name(&self) -> &str;
    fn observing(&self) -> SharedReadGuard<'_, Vec<ObservableId>>;
    fn observing_mut(&self) -> SharedWriteGuard<'_, Vec<ObservableId>>;
    fn new_observing(&self) -> SharedWriteGuard<'_, Vec<ObservableId>>;
    fn set_new_observing(&self, deps: Vec<ObservableId>);
    fn dependencies_state(&self) -> DerivationState;
    fn set_dependencies_state(&self, state: DerivationState);
    fn run_id(&self) -> u64;
    fn set_run_id(&self, id: u64);
    fn unbound_deps_count(&self) -> usize;
    fn set_unbound_deps_count(&self, n: usize);
    fn on_become_stale(&self);
    fn is_tracing(&self) -> TraceMode;
    fn requires_observable(&self) -> bool;

    fn replace_observing(&self, new: Vec<ObservableId>) -> Vec<ObservableId> {
        let mut observing = self.observing_mut();
        std::mem::replace(&mut *observing, new)
    }

    fn take_new_observing(&self) -> Vec<ObservableId> {
        let mut new_observing = self.new_observing();
        std::mem::take(&mut *new_observing)
    }
}

#[cfg(feature = "sync")]
fn derivation_registry() -> &'static Mutex<IdRegistry<DerivationId, DynDerivation>> {
    static REGISTRY: OnceLock<Mutex<IdRegistry<DerivationId, DynDerivation>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(IdRegistry::new()))
}

#[cfg(not(feature = "sync"))]
thread_local! {
    static DERIVATION_REGISTRY: RefCell<IdRegistry<DerivationId, DynDerivation>> =
        RefCell::new(IdRegistry::new());
}

pub(crate) fn reserve_derivation_id() -> DerivationId {
    #[cfg(feature = "sync")]
    {
        let mut registry = derivation_registry().lock();
        return registry.reserve();
    }

    #[cfg(not(feature = "sync"))]
    {
        return DERIVATION_REGISTRY.with(|registry| registry.borrow_mut().reserve());
    }
}

pub(crate) fn attach_derivation(id: DerivationId, derivation: &DerivationPtr) {
    #[cfg(feature = "sync")]
    {
        derivation_registry().lock().attach(id, derivation);
    }

    #[cfg(not(feature = "sync"))]
    {
        DERIVATION_REGISTRY.with(|registry| registry.borrow_mut().attach(id, derivation));
    }
}

pub(crate) fn with_derivation<R>(
    id: DerivationId,
    f: impl FnOnce(&DynDerivation) -> R,
) -> Option<R> {
    #[cfg(feature = "sync")]
    {
        let derivation = {
            let registry = derivation_registry().lock();
            registry.get(id)
        };
        derivation.map(|derivation| f(derivation.as_ref()))
    }

    #[cfg(not(feature = "sync"))]
    {
        DERIVATION_REGISTRY
            .with(|registry| registry.borrow().get(id))
            .map(|derivation| f(derivation.as_ref()))
    }
}

/// Tracks dependencies for a derivation by executing the provided function.
pub(crate) fn track_derived_function<R>(
    derivation: &DerivationPtr,
    execute: impl FnOnce() -> R,
) -> R {
    let capacity = derivation.observing().len();
    derivation.set_new_observing(Vec::with_capacity(capacity));
    derivation.set_unbound_deps_count(0);
    derivation.set_run_id(runtime::with_runtime(|runtime| runtime.next_run_id()));
    derivation.set_dependencies_state(DerivationState::UpToDate);

    let _allow_reads_guard = runtime::allow_state_reads_guard(true);
    let previous_tracking = runtime::set_tracking_derivation(Some(shared::downgrade(derivation)));

    let result = execute();

    runtime::set_tracking_derivation(previous_tracking);
    bind_dependencies(derivation);
    result
}

/// Diff the collected dependencies and update observable relationships.
pub(crate) fn bind_dependencies(derivation: &DerivationPtr) {
    let derivation_id = derivation.id();
    let mut new_dependencies = derivation.take_new_observing();
    let mut seen = HashSet::new();
    new_dependencies.retain(|id| seen.insert(*id));

    let previous_observing = derivation.replace_observing(new_dependencies.clone());
    derivation.set_unbound_deps_count(new_dependencies.len());

    for observable_id in &previous_observing {
        if !new_dependencies.contains(observable_id) {
            let _ = observable::with_observable(*observable_id, |observable| {
                observable.remove_observer(derivation_id);
            });
        }
    }

    for observable_id in &new_dependencies {
        let _ = observable::with_observable(*observable_id, |observable| {
            observable.add_observer(derivation_id);
        });
    }

    derivation.set_new_observing(previous_observing);

    if derivation.requires_observable() && new_dependencies.is_empty() {
        derivation.set_dependencies_state(DerivationState::NotTracking);
    }
}

/// Determines whether the derivation should re-run based on dependency state.
pub(crate) fn should_compute(derivation: &dyn Derivation) -> bool {
    match derivation.dependencies_state() {
        DerivationState::NotTracking => true,
        DerivationState::UpToDate => false,
        DerivationState::PossiblyStale => {
            derivation.set_dependencies_state(DerivationState::Stale);
            true
        }
        DerivationState::Stale => true,
    }
}

#[cfg(all(test, not(feature = "sync")))]
mod tests {
    use super::*;
    use crate::core::observable::{Atom, ObservableCore};
    use crate::core::runtime;
    use crate::internal::ids::DerivationId;
    use crate::internal::shared::{SharedCell, SharedReadGuard, SharedWriteGuard, new_shared};
    use std::cell::Cell;
    use std::num::NonZeroU64;

    struct TestDerivation {
        id: DerivationId,
        name: String,
        observing: SharedCell<Vec<ObservableId>>,
        new_observing: SharedCell<Vec<ObservableId>>,
        dependencies_state: Cell<DerivationState>,
        run_id: Cell<u64>,
        unbound_deps_count: Cell<usize>,
        stale_calls: Cell<u32>,
        requires_observable: bool,
    }

    impl TestDerivation {
        fn new(id: u64, name: &str) -> DerivationPtr {
            let id =
                DerivationId::from(NonZeroU64::new(id).expect("derivation id must be non-zero"));
            new_shared(Self {
                id,
                name: name.to_owned(),
                observing: SharedCell::new(Vec::new()),
                new_observing: SharedCell::new(Vec::new()),
                dependencies_state: Cell::new(DerivationState::NotTracking),
                run_id: Cell::new(0),
                unbound_deps_count: Cell::new(0),
                stale_calls: Cell::new(0),
                requires_observable: false,
            })
        }
    }

    impl Derivation for TestDerivation {
        fn id(&self) -> DerivationId {
            self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn observing(&self) -> SharedReadGuard<'_, Vec<ObservableId>> {
            self.observing.borrow()
        }

        fn observing_mut(&self) -> SharedWriteGuard<'_, Vec<ObservableId>> {
            self.observing.borrow_mut()
        }

        fn new_observing(&self) -> SharedWriteGuard<'_, Vec<ObservableId>> {
            self.new_observing.borrow_mut()
        }

        fn set_new_observing(&self, deps: Vec<ObservableId>) {
            *self.new_observing.borrow_mut() = deps;
        }

        fn dependencies_state(&self) -> DerivationState {
            self.dependencies_state.get()
        }

        fn set_dependencies_state(&self, state: DerivationState) {
            self.dependencies_state.set(state);
        }

        fn run_id(&self) -> u64 {
            self.run_id.get()
        }

        fn set_run_id(&self, id: u64) {
            self.run_id.set(id);
        }

        fn unbound_deps_count(&self) -> usize {
            self.unbound_deps_count.get()
        }

        fn set_unbound_deps_count(&self, n: usize) {
            self.unbound_deps_count.set(n);
        }

        fn on_become_stale(&self) {
            self.stale_calls.set(self.stale_calls.get() + 1);
        }

        fn is_tracing(&self) -> TraceMode {
            TraceMode::None
        }

        fn requires_observable(&self) -> bool {
            self.requires_observable
        }
    }

    fn reset_runtime() {
        runtime::with_runtime(|runtime| runtime.reset());
    }

    #[test]
    fn test_track_collects_observables() {
        reset_runtime();
        let atom = Atom::new("atom", None, None);
        let derivation = TestDerivation::new(1, "derivation");
        let derivation_dyn: DerivationPtr = derivation.clone();

        let value = track_derived_function(&derivation_dyn, || {
            assert!(atom.report_observed());
            42
        });

        assert_eq!(value, 42);
        let observing = derivation.observing();
        assert_eq!(observing.len(), 1);
        assert_eq!(observing[0], atom.id());
        drop(observing);

        let observers = atom.observers();
        assert_eq!(observers.len(), 1);
        assert_eq!(observers[0], derivation.id());
    }

    #[test]
    fn test_track_deduplicates_dependencies() {
        reset_runtime();
        let atom = Atom::new("atom", None, None);
        let derivation = TestDerivation::new(2, "dedupe");
        let derivation_dyn: DerivationPtr = derivation.clone();

        track_derived_function(&derivation_dyn, || {
            atom.report_observed();
            atom.report_observed();
        });

        let observing = derivation.observing();
        assert_eq!(observing.len(), 1);
        assert_eq!(observing[0], atom.id());
    }

    #[test]
    fn test_bind_dependencies_removes_stale_observers() {
        reset_runtime();
        let atom_a = Atom::new("atom_a", None, None);
        let atom_b = Atom::new("atom_b", None, None);
        let derivation = TestDerivation::new(3, "switch");
        let derivation_dyn: DerivationPtr = derivation.clone();

        track_derived_function(&derivation_dyn, || {
            atom_a.report_observed();
        });

        track_derived_function(&derivation_dyn, || {
            atom_b.report_observed();
        });

        assert!(atom_a.observers().is_empty());
        let observers_b = atom_b.observers();
        assert_eq!(observers_b.len(), 1);
        assert_eq!(observers_b[0], derivation.id());

        let observing = derivation.observing();
        assert_eq!(observing.len(), 1);
        assert_eq!(observing[0], atom_b.id());
    }

    #[test]
    fn test_should_compute_follows_state() {
        let derivation = TestDerivation::new(4, "compute");
        let derivation_dyn: DerivationPtr = derivation.clone();

        derivation.set_dependencies_state(DerivationState::UpToDate);
        assert!(!should_compute(derivation_dyn.as_ref()));

        derivation.set_dependencies_state(DerivationState::NotTracking);
        assert!(should_compute(derivation_dyn.as_ref()));

        derivation.set_dependencies_state(DerivationState::Stale);
        assert!(should_compute(derivation_dyn.as_ref()));

        derivation.set_dependencies_state(DerivationState::PossiblyStale);
        assert!(should_compute(derivation_dyn.as_ref()));
        assert_eq!(derivation.dependencies_state(), DerivationState::Stale);
    }
}
