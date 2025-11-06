//! User-facing observable wrappers that expose ergonomic state containers.
//!
//! When the `sync` feature is enabled these wrappers become `Send + Sync`
//! provided the stored value implements the corresponding bounds.

use crate::core::observable::{Atom, ObservableCore};
use crate::internal::shared::{Shared, SharedCell, new_shared};
use std::fmt;

#[cfg(feature = "sync")]
type ValueGuard<'a, T> = parking_lot::RwLockWriteGuard<'a, T>;
#[cfg(not(feature = "sync"))]
type ValueGuard<'a, T> = std::cell::RefMut<'a, T>;

#[cfg(feature = "sync")]
type ValueReadGuard<'a, T> = parking_lot::RwLockReadGuard<'a, T>;
#[cfg(not(feature = "sync"))]
type ValueReadGuard<'a, T> = std::cell::Ref<'a, T>;

/// Minimal observable cell that integrates with computed values and reactions.
///
/// # Examples
///
/// ```
/// use mobx_rs::observable::value::ObservableValue;
///
/// let counter = ObservableValue::new("counter", 0);
/// assert_eq!(counter.get(), 0);
///
/// counter.set(1);
/// assert_eq!(counter.get(), 1);
/// ```
pub struct ObservableValue<T> {
    atom: Shared<Atom>,
    value: Shared<SharedCell<T>>,
}

impl<T> fmt::Debug for ObservableValue<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservableValue")
            .field("name", &self.atom.name())
            .field("value", &self.value.borrow())
            .finish()
    }
}

impl<T> ObservableValue<T> {
    /// Creates a new observable cell with the provided name and initial value.
    pub fn new(name: impl Into<String>, value: T) -> Self {
        let atom = Atom::new(name, None, None);
        let value = new_shared(SharedCell::new(value));
        Self { atom, value }
    }

    /// Returns the human-readable name associated with the observable.
    pub fn name(&self) -> &str {
        self.atom.name()
    }

    /// Executes the provided closure with mutable access to the inner value.
    /// A change notification is emitted after the closure completes.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        {
            let mut slot = self.value.borrow_mut();
            f(&mut *slot);
        }
        self.atom.report_changed();
    }

    /// Returns a clone of the current value while tracking read dependencies.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.atom.report_observed();
        self.value.borrow().clone()
    }

    /// Borrows the inner value immutably while tracking dependencies.
    pub fn borrow(&self) -> ValueReadGuard<'_, T> {
        self.atom.report_observed();
        self.value.borrow()
    }

    /// Borrows the inner value mutably and notifies observers afterwards.
    pub fn borrow_mut(&self) -> ObservableValueMut<'_, T> {
        let guard = self.value.borrow_mut();
        ObservableValueMut {
            atom: self.atom.clone(),
            guard,
        }
    }

    /// Updates the value and emits a change notification.
    pub fn set(&self, value: T) {
        *self.value.borrow_mut() = value;
        self.atom.report_changed();
    }
}

/// RAII guard that notifies observers when the mutable borrow ends.
pub struct ObservableValueMut<'a, T> {
    atom: Shared<Atom>,
    guard: ValueGuard<'a, T>,
}

impl<'a, T> fmt::Debug for ObservableValueMut<'a, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ObservableValueMut")
            .field(&*self.guard)
            .finish()
    }
}

impl<'a, T> std::ops::Deref for ObservableValueMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a, T> std::ops::DerefMut for ObservableValueMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a, T> Drop for ObservableValueMut<'a, T> {
    fn drop(&mut self) {
        self.atom.report_changed();
    }
}

impl<T> Clone for ObservableValue<T> {
    fn clone(&self) -> Self {
        Self {
            atom: self.atom.clone(),
            value: self.value.clone(),
        }
    }
}
