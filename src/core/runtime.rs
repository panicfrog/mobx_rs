#![allow(dead_code)] // Runtime scaffolding evolves across phases.

pub(crate) mod config;

use self::config::RuntimeConfig;
use crate::core::derivation::DerivationWeak;
use crate::core::reaction::scheduler::ReactionScheduler;
use crate::core::spy::{SpyEvent, SpyListener};
use crate::internal::ids::ReactionId;
#[cfg(feature = "sync")]
use parking_lot::Mutex;
use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
#[cfg(feature = "sync")]
use std::sync::OnceLock;

thread_local! {
    static RUNTIME: RefCell<MobxRuntime> = RefCell::new(MobxRuntime::default());
}

pub(crate) fn with_runtime<R>(f: impl FnOnce(&mut MobxRuntime) -> R) -> R {
    RUNTIME.with(|runtime| {
        let mut runtime = runtime.borrow_mut();
        f(&mut runtime)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnforcePolicy {
    Never,
    Observed,
    Always,
}

impl Default for EnforcePolicy {
    fn default() -> Self {
        Self::Never
    }
}

#[cfg(feature = "sync")]
struct RuntimeSharedInner {
    state: Mutex<RuntimeState>,
    config: RuntimeConfig,
}

#[cfg(feature = "sync")]
impl RuntimeSharedInner {
    fn new(config: RuntimeConfig) -> Self {
        Self {
            state: Mutex::new(RuntimeState::default()),
            config,
        }
    }
}

#[cfg(feature = "sync")]
fn global_runtime_shared() -> Arc<RuntimeSharedInner> {
    static SHARED: OnceLock<Arc<RuntimeSharedInner>> = OnceLock::new();
    Arc::clone(SHARED.get_or_init(|| Arc::new(RuntimeSharedInner::new(RuntimeConfig::default()))))
}

#[cfg(not(feature = "sync"))]
struct RuntimeSharedHandle {
    state: RuntimeState,
    config: RuntimeConfig,
}

#[cfg(feature = "sync")]
struct RuntimeSharedHandle {
    shared: Arc<RuntimeSharedInner>,
}

#[cfg(not(feature = "sync"))]
impl Default for RuntimeSharedHandle {
    fn default() -> Self {
        Self {
            state: RuntimeState::default(),
            config: RuntimeConfig::default(),
        }
    }
}

#[cfg(feature = "sync")]
impl Default for RuntimeSharedHandle {
    fn default() -> Self {
        Self {
            shared: global_runtime_shared(),
        }
    }
}

impl RuntimeSharedHandle {
    fn with_state<R>(&self, f: impl FnOnce(&RuntimeState) -> R) -> R {
        #[cfg(feature = "sync")]
        {
            let guard = self.shared.state.lock();
            f(&guard)
        }

        #[cfg(not(feature = "sync"))]
        {
            f(&self.state)
        }
    }

    fn with_state_mut<R>(&mut self, f: impl FnOnce(&mut RuntimeState) -> R) -> R {
        #[cfg(feature = "sync")]
        {
            let mut guard = self.shared.state.lock();
            f(&mut guard)
        }

        #[cfg(not(feature = "sync"))]
        {
            f(&mut self.state)
        }
    }

    fn config(&self) -> &RuntimeConfig {
        #[cfg(feature = "sync")]
        {
            &self.shared.config
        }

        #[cfg(not(feature = "sync"))]
        {
            &self.config
        }
    }

    fn reaction_scheduler(&self) -> Arc<dyn ReactionScheduler> {
        #[cfg(feature = "sync")]
        {
            self.shared.config.reaction_scheduler()
        }

        #[cfg(not(feature = "sync"))]
        {
            self.config.reaction_scheduler()
        }
    }

    fn reset(&mut self) {
        #[cfg(feature = "sync")]
        {
            let mut guard = self.shared.state.lock();
            *guard = RuntimeState::default();
        }

        #[cfg(not(feature = "sync"))]
        {
            self.state = RuntimeState::default();
        }
    }
}

struct RuntimeState {
    run_id: u64,
    in_batch: usize,
    allow_state_changes: bool,
    allow_state_reads: bool,
    enforce_actions: EnforcePolicy,
    pending_reactions: Vec<ReactionId>,
    is_running_reactions: bool,
    strict_mode_warnings: Vec<String>,
    spy_listeners: Vec<(u64, SpyListener)>,
    next_spy_listener_id: u64,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            run_id: 0,
            in_batch: 0,
            allow_state_changes: true,
            allow_state_reads: true,
            enforce_actions: EnforcePolicy::default(),
            pending_reactions: Vec::new(),
            is_running_reactions: false,
            strict_mode_warnings: Vec::new(),
            spy_listeners: Vec::new(),
            next_spy_listener_id: 1,
        }
    }
}

pub(crate) struct MobxRuntime {
    shared: RuntimeSharedHandle,
    tracking_derivation: Option<DerivationWeak>,
}

impl fmt::Debug for MobxRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.shared.with_state(|state| {
            f.debug_struct("MobxRuntime")
                .field("run_id", &state.run_id)
                .field("in_batch", &state.in_batch)
                .field("allow_state_changes", &state.allow_state_changes)
                .field("allow_state_reads", &state.allow_state_reads)
                .field("enforce_actions", &state.enforce_actions)
                .field("pending_reactions", &state.pending_reactions)
                .field("is_running_reactions", &state.is_running_reactions)
                .field("strict_mode_warnings", &state.strict_mode_warnings)
                .field("spy_listener_count", &state.spy_listeners.len())
                .finish()
        })
    }
}

impl Default for MobxRuntime {
    fn default() -> Self {
        Self {
            shared: RuntimeSharedHandle::default(),
            tracking_derivation: None,
        }
    }
}

impl MobxRuntime {
    fn with_state_mut<R>(&mut self, f: impl FnOnce(&mut RuntimeState) -> R) -> R {
        self.shared.with_state_mut(f)
    }

    fn with_state<R>(&self, f: impl FnOnce(&RuntimeState) -> R) -> R {
        self.shared.with_state(f)
    }

    fn reaction_scheduler(&self) -> Arc<dyn ReactionScheduler> {
        self.shared.reaction_scheduler()
    }

    pub(crate) fn next_run_id(&mut self) -> u64 {
        self.with_state_mut(|state| {
            state.run_id = state
                .run_id
                .checked_add(1)
                .expect("MobxRuntime run_id overflowed");
            state.run_id
        })
    }

    pub(crate) fn current_run_id(&self) -> u64 {
        self.with_state(|state| state.run_id)
    }

    pub(crate) fn swap_allow_state_changes(&mut self, allow: bool) -> bool {
        self.with_state_mut(|state| std::mem::replace(&mut state.allow_state_changes, allow))
    }

    pub(crate) fn swap_allow_state_reads(&mut self, allow: bool) -> bool {
        self.with_state_mut(|state| std::mem::replace(&mut state.allow_state_reads, allow))
    }

    pub(crate) fn is_allowing_state_changes(&self) -> bool {
        self.with_state(|state| state.allow_state_changes)
    }

    pub(crate) fn is_allowing_state_reads(&self) -> bool {
        self.with_state(|state| state.allow_state_reads)
    }

    pub(crate) fn set_enforce_actions(&mut self, policy: EnforcePolicy) {
        self.with_state_mut(|state| {
            state.enforce_actions = policy;
            state.allow_state_changes = matches!(policy, EnforcePolicy::Never);
        });
    }

    pub(crate) fn enforce_actions_policy(&self) -> EnforcePolicy {
        self.with_state(|state| state.enforce_actions)
    }

    pub(crate) fn state_change_allowed(&self, is_observed: bool) -> bool {
        if self.is_allowing_state_changes() {
            return true;
        }

        match self.enforce_actions_policy() {
            EnforcePolicy::Never => true,
            EnforcePolicy::Observed => !is_observed,
            EnforcePolicy::Always => false,
        }
    }

    pub(crate) fn evaluate_strict_mode_violation(
        &mut self,
        observable_name: &str,
        is_observed: bool,
    ) -> Option<String> {
        if self.state_change_allowed(is_observed) {
            return None;
        }

        let policy_label = match self.enforce_actions_policy() {
            EnforcePolicy::Always => "always",
            EnforcePolicy::Observed => "observed",
            EnforcePolicy::Never => return None,
        };

        self.with_state_mut(|state| {
            let message = format!(
                "strict mode ({}) violation: observable `{}` was modified outside an action",
                policy_label, observable_name
            );
            state.strict_mode_warnings.push(message.clone());
            Some(message)
        })
    }

    pub(crate) fn drain_strict_mode_warnings(&mut self) -> Vec<String> {
        self.with_state_mut(|state| std::mem::take(&mut state.strict_mode_warnings))
    }

    fn next_spy_listener_id(&mut self) -> u64 {
        self.with_state_mut(|state| {
            let id = state.next_spy_listener_id;
            state.next_spy_listener_id = state
                .next_spy_listener_id
                .checked_add(1)
                .expect("spy listener id overflow");
            id
        })
    }

    pub(crate) fn add_spy_listener(&mut self, listener: SpyListener) -> u64 {
        let id = self.next_spy_listener_id();
        self.with_state_mut(|state| {
            state.spy_listeners.push((id, listener));
        });
        id
    }

    pub(crate) fn remove_spy_listener(&mut self, id: u64) -> bool {
        self.with_state_mut(|state| {
            if let Some(index) = state
                .spy_listeners
                .iter()
                .position(|(entry_id, _)| *entry_id == id)
            {
                state.spy_listeners.remove(index);
                true
            } else {
                false
            }
        })
    }

    pub(crate) fn has_spy_listeners(&self) -> bool {
        self.with_state(|state| !state.spy_listeners.is_empty())
    }

    pub(crate) fn spy_listeners_snapshot(&self) -> Vec<SpyListener> {
        self.with_state(|state| {
            state
                .spy_listeners
                .iter()
                .map(|(_, listener)| listener.clone())
                .collect()
        })
    }

    pub(crate) fn enter_batch(&mut self) {
        self.with_state_mut(|state| {
            state.in_batch = state.in_batch.saturating_add(1);
        });
    }

    pub(crate) fn exit_batch(&mut self) {
        self.with_state_mut(|state| {
            assert!(state.in_batch > 0, "attempted to exit batch with depth 0");
            state.in_batch -= 1;
        });
    }

    pub(crate) fn batch_depth(&self) -> usize {
        self.with_state(|state| state.in_batch)
    }

    pub(crate) fn reset(&mut self) {
        self.shared.reset();
        self.tracking_derivation = None;
    }

    pub(crate) fn swap_tracking_derivation(
        &mut self,
        derivation: Option<DerivationWeak>,
    ) -> Option<DerivationWeak> {
        std::mem::replace(&mut self.tracking_derivation, derivation)
    }

    pub(crate) fn tracking_derivation(&self) -> Option<DerivationWeak> {
        self.tracking_derivation.as_ref().map(Clone::clone)
    }

    pub(crate) fn enqueue_reaction(&mut self, reaction: ReactionId) {
        self.with_state_mut(|state| {
            if !state.pending_reactions.contains(&reaction) {
                state.pending_reactions.push(reaction);
            }
        });
    }

    pub(crate) fn next_pending_reaction(&mut self) -> Option<ReactionId> {
        self.with_state_mut(|state| {
            if state.pending_reactions.is_empty() {
                None
            } else {
                Some(state.pending_reactions.remove(0))
            }
        })
    }

    pub(crate) fn clear_pending_reactions(&mut self) {
        self.with_state_mut(|state| state.pending_reactions.clear());
    }

    pub(crate) fn has_pending_reactions(&self) -> bool {
        self.with_state(|state| !state.pending_reactions.is_empty())
    }

    pub(crate) fn set_running_reactions(&mut self, running: bool) -> bool {
        self.with_state_mut(|state| std::mem::replace(&mut state.is_running_reactions, running))
    }

    pub(crate) fn is_running_reactions(&self) -> bool {
        self.with_state(|state| state.is_running_reactions)
    }
}

#[must_use = "Holding the guard keeps the temporary override active"]
pub(crate) struct AllowStateChangesGuard {
    previous: bool,
}

impl AllowStateChangesGuard {
    pub(crate) fn new(allow: bool) -> Self {
        let previous = with_runtime(|runtime| runtime.swap_allow_state_changes(allow));
        Self { previous }
    }
}

impl Drop for AllowStateChangesGuard {
    fn drop(&mut self) {
        with_runtime(|runtime| {
            runtime.swap_allow_state_changes(self.previous);
        });
    }
}

#[must_use = "Holding the guard keeps the temporary override active"]
pub(crate) struct AllowStateReadsGuard {
    previous: bool,
}

impl AllowStateReadsGuard {
    pub(crate) fn new(allow: bool) -> Self {
        let previous = with_runtime(|runtime| runtime.swap_allow_state_reads(allow));
        Self { previous }
    }
}

impl Drop for AllowStateReadsGuard {
    fn drop(&mut self) {
        with_runtime(|runtime| {
            runtime.swap_allow_state_reads(self.previous);
        });
    }
}

#[must_use = "BatchGuard decrements the batch depth when dropped"]
pub(crate) struct BatchGuard {
    active: bool,
}

impl BatchGuard {
    pub(crate) fn new() -> Self {
        with_runtime(|runtime| runtime.enter_batch());
        Self { active: true }
    }

    pub(crate) fn end(mut self) {
        self.deactivate();
    }

    fn deactivate(&mut self) {
        if self.active {
            with_runtime(|runtime| runtime.exit_batch());
            self.active = false;
        }
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        self.deactivate();
    }
}

pub(crate) fn allow_state_changes_guard(allow: bool) -> AllowStateChangesGuard {
    AllowStateChangesGuard::new(allow)
}

pub(crate) fn allow_state_reads_guard(allow: bool) -> AllowStateReadsGuard {
    AllowStateReadsGuard::new(allow)
}

pub(crate) fn start_batch() -> BatchGuard {
    BatchGuard::new()
}

pub(crate) fn batch_depth() -> usize {
    with_runtime(|runtime| runtime.batch_depth())
}

pub(crate) fn state_changes_allowed() -> bool {
    with_runtime(|runtime| runtime.is_allowing_state_changes())
}

pub(crate) fn state_reads_allowed() -> bool {
    with_runtime(|runtime| runtime.is_allowing_state_reads())
}

pub(crate) fn set_tracking_derivation(
    derivation: Option<DerivationWeak>,
) -> Option<DerivationWeak> {
    with_runtime(|runtime| runtime.swap_tracking_derivation(derivation))
}

pub(crate) fn current_tracking_derivation() -> Option<DerivationWeak> {
    with_runtime(|runtime| runtime.tracking_derivation())
}

pub(crate) fn next_run_id() -> u64 {
    with_runtime(|runtime| runtime.next_run_id())
}

pub(crate) fn current_run_id() -> u64 {
    with_runtime(|runtime| runtime.current_run_id())
}

pub(crate) fn set_enforce_policy(policy: EnforcePolicy) {
    with_runtime(|runtime| runtime.set_enforce_actions(policy));
}

pub(crate) fn enforce_policy() -> EnforcePolicy {
    with_runtime(|runtime| runtime.enforce_actions_policy())
}

pub(crate) fn is_state_change_allowed(is_observed: bool) -> bool {
    with_runtime(|runtime| runtime.state_change_allowed(is_observed))
}

pub(crate) fn record_strict_mode_violation(
    observable_name: &str,
    is_observed: bool,
) -> Option<String> {
    with_runtime(|runtime| runtime.evaluate_strict_mode_violation(observable_name, is_observed))
}

pub(crate) fn drain_strict_mode_warnings() -> Vec<String> {
    with_runtime(|runtime| runtime.drain_strict_mode_warnings())
}

pub(crate) fn add_spy_listener(listener: SpyListener) -> u64 {
    with_runtime(|runtime| runtime.add_spy_listener(listener))
}

pub(crate) fn remove_spy_listener(id: u64) -> bool {
    with_runtime(|runtime| runtime.remove_spy_listener(id))
}

pub(crate) fn has_spy_listeners() -> bool {
    with_runtime(|runtime| runtime.has_spy_listeners())
}

pub(crate) fn emit_spy_event(event: SpyEvent) {
    let listeners = with_runtime(|runtime| runtime.spy_listeners_snapshot());
    if listeners.is_empty() {
        return;
    }

    for listener in listeners {
        let mut listener_ref = listener.borrow_mut();
        listener_ref.as_mut()(&event);
    }
}

pub(crate) fn enqueue_reaction(reaction: ReactionId) {
    with_runtime(|runtime| runtime.enqueue_reaction(reaction));
}

pub(crate) fn next_pending_reaction() -> Option<ReactionId> {
    with_runtime(|runtime| runtime.next_pending_reaction())
}

pub(crate) fn has_pending_reactions() -> bool {
    with_runtime(|runtime| runtime.has_pending_reactions())
}

pub(crate) fn set_running_reactions(running: bool) -> bool {
    with_runtime(|runtime| runtime.set_running_reactions(running))
}

pub(crate) fn is_running_reactions() -> bool {
    with_runtime(|runtime| runtime.is_running_reactions())
}

pub(crate) fn clear_pending_reactions() {
    with_runtime(|runtime| runtime.clear_pending_reactions());
}

pub(crate) fn schedule_reaction_flush() {
    let scheduler = with_runtime(|runtime| runtime.reaction_scheduler());
    scheduler.schedule();
}

#[cfg(test)]
fn reset_runtime() {
    with_runtime(|runtime| runtime.reset());
}

#[cfg(all(test, not(feature = "sync")))]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    #[test]
    fn test_state_change_guard_restores_previous_value() {
        reset_runtime();
        assert!(state_changes_allowed());

        {
            let _guard = allow_state_changes_guard(false);
            assert!(!state_changes_allowed());
        }

        assert!(state_changes_allowed());
    }

    #[test]
    fn test_state_read_guard_restores_previous_value() {
        reset_runtime();
        assert!(state_reads_allowed());

        {
            let _guard = allow_state_reads_guard(false);
            assert!(!state_reads_allowed());
        }

        assert!(state_reads_allowed());
    }

    #[test]
    fn test_batch_guard_tracks_depth() {
        reset_runtime();
        assert_eq!(batch_depth(), 0);

        let outer = start_batch();
        assert_eq!(batch_depth(), 1);

        {
            let _inner = start_batch();
            assert_eq!(batch_depth(), 2);
        }

        assert_eq!(batch_depth(), 1);
        drop(outer);
        assert_eq!(batch_depth(), 0);
    }

    #[test]
    fn test_manual_batch_end() {
        reset_runtime();
        assert_eq!(batch_depth(), 0);

        let guard = start_batch();
        assert_eq!(batch_depth(), 1);
        guard.end();
        assert_eq!(batch_depth(), 0);
    }

    #[test]
    fn test_run_id_monotonicity() {
        reset_runtime();
        let first = next_run_id();
        let second = next_run_id();
        assert_eq!(first + 1, second);
        assert_eq!(current_run_id(), second);
    }

    #[test]
    fn test_enforce_policy_assignment() {
        reset_runtime();
        assert_eq!(enforce_policy(), EnforcePolicy::Never);
        set_enforce_policy(EnforcePolicy::Always);
        assert_eq!(enforce_policy(), EnforcePolicy::Always);
    }

    #[test]
    fn test_enqueue_reaction_tracks_uniqueness() {
        reset_runtime();
        let reaction = ReactionId::from(NonZeroU64::new(1).unwrap());
        enqueue_reaction(reaction);
        enqueue_reaction(reaction);
        assert!(has_pending_reactions());
        assert_eq!(next_pending_reaction(), Some(reaction));
        assert!(!has_pending_reactions());
    }

    #[test]
    fn test_running_reactions_flag() {
        reset_runtime();
        assert!(!is_running_reactions());
        let previous = set_running_reactions(true);
        assert!(!previous);
        assert!(is_running_reactions());
        let previous = set_running_reactions(false);
        assert!(previous);
        assert!(!is_running_reactions());
    }

    #[test]
    fn test_state_change_allowed_respects_policy() {
        reset_runtime();
        set_enforce_policy(EnforcePolicy::Observed);
        assert!(is_state_change_allowed(false));
        assert!(!is_state_change_allowed(true));
    }

    #[test]
    fn test_record_strict_mode_violation_tracks_warning() {
        reset_runtime();
        set_enforce_policy(EnforcePolicy::Always);
        let warning = record_strict_mode_violation("test_atom", true);
        assert!(warning.is_some());
        let warnings = drain_strict_mode_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("test_atom"));
    }
}
