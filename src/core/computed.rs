#![allow(dead_code)] // Computed will expose additional APIs in later phases.

//! Computed values provide memoized, derivation-backed observables.

use crate::core::derivation::{
    self, Derivation, DerivationPtr, DerivationState, TraceMode, bind_dependencies,
    reserve_derivation_id, should_compute,
};
use crate::core::observable::{self, DynObservableCore, ObservableCore};
use crate::core::runtime;
use crate::core::spy::{self, SpyEvent};
use crate::internal::ids::{DerivationId, ObservableId};
use crate::internal::shared::{
    Shared, SharedCell, SharedReadGuard, SharedWeak, SharedWriteGuard, new_cyclic,
};
use std::fmt;

#[cfg(feature = "sync")]
type ComputedGetter<T> = dyn Fn() -> T + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type ComputedGetter<T> = dyn Fn() -> T + 'static;

#[cfg(feature = "sync")]
type ComputedSetter<T> = dyn FnMut(T) + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type ComputedSetter<T> = dyn FnMut(T) + 'static;

#[cfg(feature = "sync")]
type ComputedEquals<T> = dyn Fn(&T, &T) -> bool + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type ComputedEquals<T> = dyn Fn(&T, &T) -> bool + 'static;

#[cfg(feature = "sync")]
pub trait ComputedValue: Clone + PartialEq + Send + Sync + 'static {}

#[cfg(feature = "sync")]
impl<T> ComputedValue for T where T: Clone + PartialEq + Send + Sync + 'static {}

#[cfg(not(feature = "sync"))]
pub trait ComputedValue: Clone + PartialEq + 'static {}

#[cfg(not(feature = "sync"))]
impl<T> ComputedValue for T where T: Clone + PartialEq + 'static {}

/// Configuration options for creating a computed value.
pub struct ComputedOptions<T> {
    name: Option<String>,
    getter: Box<ComputedGetter<T>>,
    setter: Option<Box<ComputedSetter<T>>>,
    equals: Option<Box<ComputedEquals<T>>>,
    keep_alive: bool,
    requires_reaction: bool,
}

impl<T> ComputedOptions<T> {
    /// Creates a new options builder with the provided getter function.
    #[cfg(feature = "sync")]
    pub fn new(getter: impl Fn() -> T + Send + Sync + 'static) -> Self {
        Self {
            name: None,
            getter: Box::new(getter),
            setter: None,
            equals: None,
            keep_alive: false,
            requires_reaction: false,
        }
    }

    /// Creates a new options builder with the provided getter function.
    #[cfg(not(feature = "sync"))]
    pub fn new(getter: impl Fn() -> T + 'static) -> Self {
        Self {
            name: None,
            getter: Box::new(getter),
            setter: None,
            equals: None,
            keep_alive: false,
            requires_reaction: false,
        }
    }

    /// Sets the human readable name for diagnostics.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the setter used when the computed value is assigned to.
    #[cfg(feature = "sync")]
    pub fn setter(mut self, setter: impl FnMut(T) + Send + Sync + 'static) -> Self {
        self.setter = Some(Box::new(setter));
        self
    }

    /// Sets the setter used when the computed value is assigned to.
    #[cfg(not(feature = "sync"))]
    pub fn setter(mut self, setter: impl FnMut(T) + 'static) -> Self {
        self.setter = Some(Box::new(setter));
        self
    }

    /// Overrides the equality comparator that determines cache hits.
    #[cfg(feature = "sync")]
    pub fn equals(mut self, equals: impl Fn(&T, &T) -> bool + Send + Sync + 'static) -> Self {
        self.equals = Some(Box::new(equals));
        self
    }

    /// Overrides the equality comparator that determines cache hits.
    #[cfg(not(feature = "sync"))]
    pub fn equals(mut self, equals: impl Fn(&T, &T) -> bool + 'static) -> Self {
        self.equals = Some(Box::new(equals));
        self
    }

    /// Forces the computed to stay active even when no observers are attached.
    pub fn keep_alive(mut self, keep_alive: bool) -> Self {
        self.keep_alive = keep_alive;
        self
    }

    /// Marks the computed as requiring a reaction before being considered valid.
    pub fn requires_reaction(mut self, requires_reaction: bool) -> Self {
        self.requires_reaction = requires_reaction;
        self
    }
}

/// Public handle that provides read/write access to a computed value.
#[derive(Clone)]
pub struct Computed<T>
where
    T: ComputedValue,
{
    inner: Shared<ComputedInner<T>>,
}

impl<T> Computed<T>
where
    T: ComputedValue,
{
    /// Constructs a computed value from the provided options.
    pub fn new(options: ComputedOptions<T>) -> Self {
        let inner = ComputedInner::new(options);
        Self { inner }
    }

    /// Evaluates the computed value, tracking dependencies when applicable.
    pub fn get(&self) -> T {
        ComputedInner::get(&self.inner)
    }

    /// Returns the cached value if present without triggering dependency tracking.
    pub fn peek(&self) -> Option<T> {
        self.inner.peek()
    }

    /// Attempts to update the computed via its setter.
    pub fn set(&self, value: T) -> Result<(), ComputedSetError> {
        self.inner.set(value)
    }

    /// Returns the observable identifier backing this computed value.
    pub(crate) fn observable_id(&self) -> ObservableId {
        self.inner.observable_id
    }

    /// Returns the derivation identifier associated with this computed value.
    pub(crate) fn derivation_id(&self) -> DerivationId {
        self.inner.derivation_id
    }
}

impl<T> fmt::Debug for Computed<T>
where
    T: ComputedValue + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Computed")
            .field("name", &self.inner.name)
            .field("observable_id", &self.inner.observable_id)
            .field("derivation_id", &self.inner.derivation_id)
            .finish()
    }
}

/// Error returned when attempting to set a computed value without a setter.
#[derive(Debug, thiserror::Error)]
#[error("computed value `{name}` does not provide a setter")]
pub struct ComputedSetError {
    name: String,
}

struct ComputedInner<T>
where
    T: ComputedValue,
{
    observable_id: ObservableId,
    derivation_id: DerivationId,
    name: String,
    observers: SharedCell<Vec<DerivationId>>,
    diff_value: SharedCell<u8>,
    is_being_observed: SharedCell<bool>,
    is_pending_unobservation: SharedCell<bool>,
    lowest_observer_state: SharedCell<DerivationState>,
    value: SharedCell<Option<T>>,
    getter: SharedCell<Box<ComputedGetter<T>>>,
    setter: SharedCell<Option<Box<ComputedSetter<T>>>>,
    equals: Box<ComputedEquals<T>>,
    observing: SharedCell<Vec<ObservableId>>,
    new_observing: SharedCell<Vec<ObservableId>>,
    dependencies_state: SharedCell<DerivationState>,
    run_id: SharedCell<u64>,
    unbound_deps_count: SharedCell<usize>,
    is_computing: SharedCell<bool>,
    keep_alive: bool,
    requires_reaction: bool,
    self_ref: SharedWeak<ComputedInner<T>>,
}

impl<T> ComputedInner<T>
where
    T: ComputedValue,
{
    fn new(mut options: ComputedOptions<T>) -> Shared<Self> {
        let observable_id = observable::reserve_observable_id();
        let derivation_id = reserve_derivation_id();
        let name = options
            .name
            .take()
            .unwrap_or_else(|| format!("Computed@{}", u64::from(derivation_id)));
        let getter = options.getter;
        let setter = options.setter.take();
        let equals = options
            .equals
            .take()
            .unwrap_or_else(|| Box::new(|a: &T, b: &T| a == b));
        let keep_alive = options.keep_alive;
        let requires_reaction = options.requires_reaction;

        let inner = new_cyclic(|weak| Self {
            observable_id,
            derivation_id,
            name,
            observers: SharedCell::new(Vec::new()),
            diff_value: SharedCell::new(0),
            is_being_observed: SharedCell::new(false),
            is_pending_unobservation: SharedCell::new(false),
            lowest_observer_state: SharedCell::new(DerivationState::UpToDate),
            value: SharedCell::new(None),
            getter: SharedCell::new(getter),
            setter: SharedCell::new(setter),
            equals,
            observing: SharedCell::new(Vec::new()),
            new_observing: SharedCell::new(Vec::new()),
            dependencies_state: SharedCell::new(DerivationState::NotTracking),
            run_id: SharedCell::new(0),
            unbound_deps_count: SharedCell::new(0),
            is_computing: SharedCell::new(false),
            keep_alive,
            requires_reaction,
            self_ref: weak,
        });

        let observable_trait: Shared<DynObservableCore> = inner.clone();
        observable::attach_observable(observable_id, &observable_trait);
        let derivation_trait: DerivationPtr = inner.clone();
        derivation::attach_derivation(derivation_id, &derivation_trait);

        inner
    }

    fn get(this: &Shared<Self>) -> T {
        this.report_observed();

        if should_compute(this.as_ref()) {
            this.track_and_compute();
        }

        this.value
            .borrow()
            .as_ref()
            .expect("computed value should be initialized after compute")
            .clone()
    }

    fn peek(&self) -> Option<T> {
        self.value.borrow().as_ref().cloned()
    }

    fn set(&self, value: T) -> Result<(), ComputedSetError> {
        if let Some(setter) = self.setter.borrow_mut().as_mut() {
            setter(value);
            Ok(())
        } else {
            Err(ComputedSetError {
                name: self.name.clone(),
            })
        }
    }

    fn track_and_compute(&self) {
        let Some(this) = self.upgrade() else {
            return;
        };

        let emit_spy = spy::is_enabled();
        if emit_spy {
            spy::report(SpyEvent::ComputedStart {
                name: this.name.clone(),
                derivation_id: u64::from(this.derivation_id),
            });
        }

        {
            let mut computing = this.is_computing.borrow_mut();
            if *computing {
                panic!(
                    "recursive computation detected for computed `{}`",
                    this.name
                );
            }
            *computing = true;
        }

        let derivation_obj: DerivationPtr = this.clone();
        let result = derivation::track_derived_function(&derivation_obj, || {
            let getter = this.getter.borrow();
            (getter.as_ref())()
        });

        *this.is_computing.borrow_mut() = false;

        let mut value_slot = this.value.borrow_mut();
        let changed = match value_slot.as_ref() {
            Some(previous) => !(this.equals)(previous, &result),
            None => true,
        };

        *value_slot = Some(result);

        *this.dependencies_state.borrow_mut() = DerivationState::UpToDate;
        *this.run_id.borrow_mut() = runtime::current_run_id();

        if changed {
            this.propagate_change();
        }

        if !this.keep_alive && !*this.is_being_observed.borrow() {
            ComputedInner::suspend(&this);
        }

        if emit_spy {
            spy::report(SpyEvent::ComputedEnd {
                name: this.name.clone(),
                derivation_id: u64::from(this.derivation_id),
                changed,
            });
        }
    }

    fn propagate_change(&self) {
        *self.lowest_observer_state.borrow_mut() = DerivationState::Stale;
        let current = *self.diff_value.borrow();
        *self.diff_value.borrow_mut() = current.wrapping_add(1);

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

    fn suspend(this: &Shared<Self>) {
        if this.keep_alive {
            return;
        }

        this.value.borrow_mut().take();
        *this.dependencies_state.borrow_mut() = DerivationState::NotTracking;

        this.set_new_observing(Vec::new());
        let derivation_obj: DerivationPtr = this.clone();
        bind_dependencies(&derivation_obj);
    }

    fn upgrade(&self) -> Option<Shared<Self>> {
        self.self_ref.upgrade()
    }
}

impl<T> ObservableCore for ComputedInner<T>
where
    T: ComputedValue,
{
    fn id(&self) -> ObservableId {
        self.observable_id
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
            new_observing.push(self.observable_id);
            derivation.set_unbound_deps_count(new_observing.len());
        }

        if !*self.is_being_observed.borrow() {
            *self.is_being_observed.borrow_mut() = true;
            *self.is_pending_unobservation.borrow_mut() = false;
        }

        if should_compute(self) {
            if let Some(this) = self.upgrade() {
                this.track_and_compute();
            }
        }

        true
    }

    fn add_observer(&self, derivation: DerivationId) {
        let mut observers = self.observers.borrow_mut();
        if !observers.contains(&derivation) {
            observers.push(derivation);
        }
        *self.is_being_observed.borrow_mut() = true;
        *self.is_pending_unobservation.borrow_mut() = false;
    }

    fn remove_observer(&self, derivation: DerivationId) {
        let mut observers = self.observers.borrow_mut();
        if let Some(index) = observers.iter().position(|id| *id == derivation) {
            observers.swap_remove(index);
        }

        if observers.is_empty() {
            *self.is_being_observed.borrow_mut() = false;
            *self.is_pending_unobservation.borrow_mut() = true;

            if let Some(this) = self.upgrade() {
                ComputedInner::suspend(&this);
            }
        }
    }

    fn report_changed(&self) {
        self.propagate_change();
    }

    fn on_become_observed(&self) {}

    fn on_become_unobserved(&self) {}

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

impl<T> Derivation for ComputedInner<T>
where
    T: ComputedValue,
{
    fn id(&self) -> DerivationId {
        self.derivation_id
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
        *self.dependencies_state.borrow()
    }

    fn set_dependencies_state(&self, state: DerivationState) {
        *self.dependencies_state.borrow_mut() = state;
    }

    fn run_id(&self) -> u64 {
        *self.run_id.borrow()
    }

    fn set_run_id(&self, id: u64) {
        *self.run_id.borrow_mut() = id;
    }

    fn unbound_deps_count(&self) -> usize {
        *self.unbound_deps_count.borrow()
    }

    fn set_unbound_deps_count(&self, n: usize) {
        *self.unbound_deps_count.borrow_mut() = n;
    }

    fn on_become_stale(&self) {
        *self.dependencies_state.borrow_mut() = DerivationState::Stale;
        self.propagate_change();
    }

    fn is_tracing(&self) -> TraceMode {
        TraceMode::None
    }

    fn requires_observable(&self) -> bool {
        self.requires_reaction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::observable::Atom;
    use crate::internal::shared::{Shared, SharedCell, new_shared};

    struct ObservableBox<T>
    where
        T: Copy,
    {
        atom: Shared<Atom>,
        value: SharedCell<T>,
    }

    impl<T> ObservableBox<T>
    where
        T: Copy,
    {
        fn new(initial: T) -> Self {
            Self {
                atom: Atom::new("observable_box", None, None),
                value: SharedCell::new(initial),
            }
        }

        fn get(&self) -> T {
            self.atom.report_observed();
            *self.value.borrow()
        }

        fn set(&self, value: T) {
            *self.value.borrow_mut() = value;
            self.atom.report_changed();
        }
    }

    #[test]
    fn test_computed_tracks_and_recomputes() {
        runtime::with_runtime(|runtime| runtime.reset());
        let source = new_shared(ObservableBox::new(1));
        let compute_count = new_shared(SharedCell::new(0u32));
        let compute_count_clone = compute_count.clone();
        let source_clone = source.clone();

        let computed = Computed::new(ComputedOptions::new(move || {
            {
                let mut count = compute_count_clone.borrow_mut();
                *count += 1;
            }
            source_clone.get() * 2
        }));

        assert_eq!(computed.get(), 2);
        assert_eq!(*compute_count.borrow(), 1);

        // Cache should be reused without recomputing.
        assert_eq!(computed.get(), 2);
        assert_eq!(*compute_count.borrow(), 1);

        source.set(3);
        assert_eq!(computed.get(), 6);
        assert_eq!(*compute_count.borrow(), 2);
    }

    #[test]
    fn test_computed_setter_invocation() {
        runtime::with_runtime(|runtime| runtime.reset());
        let sink = new_shared(SharedCell::new(0));
        let sink_clone = sink.clone();

        let computed = Computed::new(ComputedOptions::new(|| 7).setter(move |value| {
            *sink_clone.borrow_mut() = value;
        }));

        computed.set(42).unwrap();
        assert_eq!(*sink.borrow(), 42);
    }

    #[test]
    fn test_computed_set_without_setter_fails() {
        runtime::with_runtime(|runtime| runtime.reset());
        let computed = Computed::new(ComputedOptions::new(|| 5));
        let err = computed.set(10).unwrap_err();
        assert!(
            err.name.starts_with("Computed@"),
            "expected computed name to follow Computed@* pattern, got {}",
            err.name
        );
    }
}
