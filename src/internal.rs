//! Internal building blocks that power the public MobX runtime API.
//!
//! The internal module tree hosts ID newtypes and registries that enable the
//! runtime to track observables, derivations, and reactions without exposing
//! the implementation details to end users.

pub(crate) mod ids;
pub(crate) mod registry;
pub(crate) mod shared;
