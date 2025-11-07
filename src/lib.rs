#![cfg_attr(not(feature = "ffi"), forbid(unsafe_code))]

//! Core crate scaffolding for the MobX-inspired reactivity runtime.
//!
//! Phase 0 establishes the foundational module layout and ID registry utilities
//! required by subsequent runtime components.

pub(crate) mod core;
#[cfg(feature = "ffi")]
pub mod ffi;
pub(crate) mod internal;
mod macros;
pub mod observable;

pub use crate::core::action::{
    ActionPolicy, action, allow_state_changes, drain_strict_mode_warnings, enforce_actions_policy,
    run_in_action, set_enforce_actions,
};
pub use crate::core::computed::{Computed, ComputedOptions, ComputedSetError};
pub use crate::core::reaction::{ReactionHandle, ReactionOptions, autorun, run_pending_reactions};
pub use crate::observable::collections::{
    map::ObservableMap, set::ObservableSet, vec::ObservableVec,
};
/// Spy diagnostics that allow listeners to observe runtime events such as actions,
/// observable reads/writes, and reaction lifecycles.
pub mod spy {
    pub use crate::core::spy::{SpyEvent, SpySubscription, is_enabled, register};
}
