#![allow(dead_code)] // Reaction APIs will expand in later phases.

//! Reaction scaffolding that schedules derived side-effects when observables change.

pub(crate) mod scheduler;

use crate::core::derivation::{
    self, Derivation, DerivationPtr, DerivationState, TraceMode, bind_dependencies,
    reserve_derivation_id, track_derived_function,
};
use crate::core::runtime;
use crate::core::spy::{self, SpyEvent};
use crate::internal::ids::{DerivationId, ObservableId, ReactionId};
use crate::internal::registry::IdRegistry;
use crate::internal::shared::{
    Shared, SharedCell, SharedReadGuard, SharedWeak, SharedWriteGuard, new_cyclic,
};
#[cfg(feature = "sync")]
use parking_lot::Mutex;
#[cfg(not(feature = "sync"))]
use std::cell::RefCell;
#[cfg(feature = "sync")]
use std::sync::OnceLock;

#[cfg(feature = "sync")]
type ReactionEffect = dyn FnMut() + Send + Sync + 'static;
#[cfg(not(feature = "sync"))]
type ReactionEffect = dyn FnMut() + 'static;

#[cfg(feature = "sync")]
fn reaction_registry() -> &'static Mutex<IdRegistry<ReactionId, Reaction>> {
    static REGISTRY: OnceLock<Mutex<IdRegistry<ReactionId, Reaction>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(IdRegistry::new()))
}

#[cfg(not(feature = "sync"))]
thread_local! {
    static REACTION_REGISTRY: RefCell<IdRegistry<ReactionId, Reaction>> =
        RefCell::new(IdRegistry::new());
}

pub(crate) fn reserve_reaction_id() -> ReactionId {
    #[cfg(feature = "sync")]
    {
        let mut registry = reaction_registry().lock();
        return registry.reserve();
    }

    #[cfg(not(feature = "sync"))]
    {
        return REACTION_REGISTRY.with(|registry| registry.borrow_mut().reserve());
    }
}

pub(crate) fn attach_reaction(id: ReactionId, reaction: &Shared<Reaction>) {
    #[cfg(feature = "sync")]
    {
        reaction_registry().lock().attach(id, reaction);
    }

    #[cfg(not(feature = "sync"))]
    {
        REACTION_REGISTRY.with(|registry| registry.borrow_mut().attach(id, reaction));
    }
}

pub(crate) fn with_reaction<R>(
    id: ReactionId,
    f: impl FnOnce(&Shared<Reaction>) -> R,
) -> Option<R> {
    #[cfg(feature = "sync")]
    {
        let reaction = {
            let registry = reaction_registry().lock();
            registry.get(id)
        };
        reaction.map(|reaction| f(&reaction))
    }

    #[cfg(not(feature = "sync"))]
    {
        REACTION_REGISTRY
            .with(|registry| registry.borrow().get(id))
            .map(|reaction| f(&reaction))
    }
}

/// Builder for configuring a reaction before spawning it.
pub struct ReactionOptions {
    name: Option<String>,
    effect: Box<ReactionEffect>,
    requires_observable: bool,
}

impl ReactionOptions {
    /// Creates a new reaction configuration with the mandatory effect.
    #[cfg(feature = "sync")]
    pub fn new(effect: impl FnMut() + Send + Sync + 'static) -> Self {
        Self {
            name: None,
            effect: Box::new(effect),
            requires_observable: false,
        }
    }

    #[cfg(not(feature = "sync"))]
    pub fn new(effect: impl FnMut() + 'static) -> Self {
        Self {
            name: None,
            effect: Box::new(effect),
            requires_observable: false,
        }
    }

    /// Sets the human readable name for diagnostics.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Requires at least one observable dependency to be tracked.
    pub fn requires_observable(mut self, requires: bool) -> Self {
        self.requires_observable = requires;
        self
    }
}

/// Handle to a reaction that can be scheduled or disposed.
#[derive(Clone)]
pub struct ReactionHandle {
    inner: Shared<Reaction>,
}

impl ReactionHandle {
    /// Schedules the reaction to run on the next flush cycle.
    pub fn schedule(&self) {
        self.inner.schedule();
    }

    /// Runs the reaction immediately, bypassing the queue.
    pub fn run_now(&self) {
        self.inner.run();
    }

    /// Disposes the reaction, clearing tracked dependencies.
    pub fn dispose(&self) {
        self.inner.dispose();
    }

    /// Returns whether the reaction has been disposed.
    pub fn is_disposed(&self) -> bool {
        self.inner.is_disposed()
    }
}

/// Creates a reaction that runs immediately and reschedules when dependencies change.
#[cfg(feature = "sync")]
pub fn autorun(effect: impl FnMut() + Send + Sync + 'static) -> ReactionHandle {
    let reaction = Reaction::new(ReactionOptions::new(effect));
    reaction.run();
    ReactionHandle { inner: reaction }
}

/// Creates a reaction that runs immediately and reschedules when dependencies change.
#[cfg(not(feature = "sync"))]
pub fn autorun(effect: impl FnMut() + 'static) -> ReactionHandle {
    let reaction = Reaction::new(ReactionOptions::new(effect));
    reaction.run();
    ReactionHandle { inner: reaction }
}

/// Flushes all pending reactions in FIFO order.
pub fn run_pending_reactions() {
    let already_running = runtime::set_running_reactions(true);
    if already_running {
        return;
    }

    while let Some(reaction_id) = runtime::next_pending_reaction() {
        let _ = with_reaction(reaction_id, |reaction| {
            reaction.run();
        });
    }

    runtime::set_running_reactions(false);
}

pub(crate) struct Reaction {
    reaction_id: ReactionId,
    derivation_id: DerivationId,
    name: String,
    effect: SharedCell<Box<ReactionEffect>>,
    observing: SharedCell<Vec<ObservableId>>,
    new_observing: SharedCell<Vec<ObservableId>>,
    dependencies_state: SharedCell<DerivationState>,
    run_id: SharedCell<u64>,
    unbound_deps_count: SharedCell<usize>,
    is_scheduled: SharedCell<bool>,
    is_running: SharedCell<bool>,
    is_disposed: SharedCell<bool>,
    requires_observable: bool,
    self_ref: SharedWeak<Reaction>,
}

impl Reaction {
    fn new_state<T: Default>() -> SharedCell<T> {
        SharedCell::new(T::default())
    }

    pub(crate) fn new(options: ReactionOptions) -> Shared<Self> {
        let reaction_id = reserve_reaction_id();
        let derivation_id = reserve_derivation_id();
        let name = options
            .name
            .unwrap_or_else(|| format!("Reaction@{}", u64::from(reaction_id)));
        let requires_observable = options.requires_observable;
        let effect = options.effect;

        let reaction = new_cyclic(|weak| Self {
            reaction_id,
            derivation_id,
            name,
            effect: SharedCell::new(effect),
            observing: SharedCell::new(Vec::new()),
            new_observing: SharedCell::new(Vec::new()),
            dependencies_state: SharedCell::new(DerivationState::NotTracking),
            run_id: Self::new_state(),
            unbound_deps_count: Self::new_state(),
            is_scheduled: Self::new_state(),
            is_running: Self::new_state(),
            is_disposed: Self::new_state(),
            requires_observable,
            self_ref: weak,
        });

        let derivation_trait: DerivationPtr = reaction.clone();
        derivation::attach_derivation(derivation_id, &derivation_trait);
        attach_reaction(reaction_id, &reaction);

        reaction
    }

    fn upgrade(&self) -> Option<Shared<Self>> {
        self.self_ref.upgrade()
    }

    fn swap_flag(flag: &SharedCell<bool>, value: bool) -> bool {
        let mut guard = flag.borrow_mut();
        std::mem::replace(&mut *guard, value)
    }

    fn track_effect(&self) {
        let Some(reaction) = self.upgrade() else {
            return;
        };

        {
            let mut running = reaction.is_running.borrow_mut();
            if *running {
                panic!(
                    "recursive reaction execution detected for `{}`",
                    reaction.name
                );
            }
            *running = true;
        }

        let derivation_obj: DerivationPtr = reaction.clone();
        track_derived_function(&derivation_obj, || {
            let mut effect = reaction.effect.borrow_mut();
            (effect.as_mut())();
        });

        *reaction.dependencies_state.borrow_mut() = DerivationState::UpToDate;
        *reaction.run_id.borrow_mut() = runtime::current_run_id();
        *reaction.is_running.borrow_mut() = false;
    }

    fn is_disposed(&self) -> bool {
        *self.is_disposed.borrow()
    }

    pub(crate) fn schedule(&self) {
        if self.is_disposed() {
            return;
        }
        if !Self::swap_flag(&self.is_scheduled, true) {
            runtime::enqueue_reaction(self.reaction_id);
            if spy::is_enabled() {
                spy::report(SpyEvent::ReactionScheduled {
                    name: self.name.clone(),
                    reaction_id: u64::from(self.reaction_id),
                });
            }
            runtime::schedule_reaction_flush();
        }
    }

    pub(crate) fn run(&self) {
        if self.is_disposed() {
            return;
        }

        *self.is_scheduled.borrow_mut() = false;
        let emit_spy = spy::is_enabled();
        if emit_spy {
            spy::report(SpyEvent::ReactionRunStart {
                name: self.name.clone(),
                reaction_id: u64::from(self.reaction_id),
            });
        }
        self.track_effect();

        if self.requires_observable && self.observing.borrow().is_empty() {
            self.dispose();
        }

        if emit_spy {
            spy::report(SpyEvent::ReactionRunEnd {
                name: self.name.clone(),
                reaction_id: u64::from(self.reaction_id),
            });
        }
    }

    pub(crate) fn dispose(&self) {
        if Self::swap_flag(&self.is_disposed, true) {
            return;
        }
        *self.is_scheduled.borrow_mut() = false;
        *self.dependencies_state.borrow_mut() = DerivationState::NotTracking;
        self.set_new_observing(Vec::new());

        if let Some(reaction) = self.upgrade() {
            let derivation_obj: DerivationPtr = reaction.clone();
            bind_dependencies(&derivation_obj);
        }

        self.observing.borrow_mut().clear();
        self.new_observing.borrow_mut().clear();

        if spy::is_enabled() {
            spy::report(SpyEvent::ReactionDisposed {
                name: self.name.clone(),
                reaction_id: u64::from(self.reaction_id),
            });
        }
    }
}

impl Derivation for Reaction {
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
        self.schedule();
    }

    fn is_tracing(&self) -> TraceMode {
        TraceMode::None
    }

    fn requires_observable(&self) -> bool {
        self.requires_observable
    }
}

#[cfg(all(test, not(feature = "sync")))]
mod tests {
    use super::*;
    use crate::core::observable::{Atom, ObservableCore};
    use crate::internal::shared::new_shared;
    use std::cell::Cell;
    use std::num::NonZeroU64;

    #[test]
    fn test_reaction_tracks_dependencies_and_runs() {
        runtime::with_runtime(|runtime| runtime.reset());
        let atom = Atom::new("reaction_atom", None, None);
        let counter = new_shared(Cell::new(0));
        let counter_clone = counter.clone();
        let atom_for_reaction = atom.clone();

        let handle = autorun(move || {
            atom_for_reaction.report_observed();
            counter_clone.set(counter_clone.get() + 1);
        });

        assert_eq!(counter.get(), 1);

        atom.report_changed();
        run_pending_reactions();
        assert_eq!(counter.get(), 2);

        handle.dispose();
        atom.report_changed();
        run_pending_reactions();
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn test_schedule_deduplicates_pending_reactions() {
        runtime::with_runtime(|runtime| runtime.reset());
        let reaction_id = ReactionId::from(NonZeroU64::new(1).expect("non-zero id"));
        runtime::enqueue_reaction(reaction_id);
        runtime::enqueue_reaction(reaction_id);

        let mut count = 0;
        while let Some(_) = runtime::next_pending_reaction() {
            count += 1;
        }

        assert_eq!(count, 1);
        assert!(!runtime::has_pending_reactions());
    }
}
