//! Core runtime components that implement the MobX-style reactivity system.
//!
//! The `core` module is organized into focused submodules (e.g. `runtime`) that
//! expose the building blocks used by higher-level observable and reaction
//! functionality.

pub(crate) mod action;
pub(crate) mod computed;
pub(crate) mod derivation;
pub(crate) mod observable;
pub(crate) mod reaction;
pub(crate) mod runtime;
pub(crate) mod spy;
