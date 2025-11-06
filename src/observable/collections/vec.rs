use crate::core::observable::{Atom, ObservableCore};
use crate::internal::shared::{Shared, SharedCell, new_shared};
use crate::observable::value::ObservableValue;
use std::fmt;

/// Observable wrapper around `Vec<T>` that tracks structural and per-element changes.
#[derive(Clone)]
pub struct ObservableVec<T> {
    name: String,
    structure: Shared<Atom>,
    values: Shared<SharedCell<Vec<ObservableValue<T>>>>,
}

impl<T> ObservableVec<T> {
    /// Creates a new empty observable vector with the provided name.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            structure: Atom::new(format!("{name}::structure"), None, None),
            values: new_shared(SharedCell::new(Vec::new())),
            name,
        }
    }

    /// Returns the number of elements in the vector.
    pub fn len(&self) -> usize {
        self.structure.report_observed();
        self.values.borrow().len()
    }

    /// Returns `true` if the vector contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a clone of the element at the specified index.
    pub fn get(&self, index: usize) -> Option<T>
    where
        T: Clone,
    {
        let guard = self.values.borrow();
        guard.get(index).map(|value| value.get())
    }

    /// Provides access to the element observable at the given index.
    pub fn get_observable(&self, index: usize) -> Option<ObservableValue<T>> {
        let guard = self.values.borrow();
        guard.get(index).cloned()
    }

    /// Appends a new element to the vector.
    pub fn push(&self, value: T) {
        let mut guard = self.values.borrow_mut();
        let entry = ObservableValue::new(self.element_name(guard.len()), value);
        guard.push(entry);
        drop(guard);
        self.structure.report_changed();
    }

    /// Removes the last element and returns it.
    pub fn pop(&self) -> Option<T>
    where
        T: Clone,
    {
        let mut guard = self.values.borrow_mut();
        let result = guard.pop().map(|value| value.get());
        if result.is_some() {
            drop(guard);
            self.structure.report_changed();
        }
        result
    }

    /// Sets the element at the provided index.
    pub fn set(&self, index: usize, value: T) -> Result<(), T> {
        let guard = self.values.borrow();
        let Some(entry) = guard.get(index) else {
            return Err(value);
        };
        entry.set(value);
        Ok(())
    }

    /// Inserts an element at the provided position, shifting subsequent elements to the right.
    pub fn insert(&self, index: usize, value: T) {
        let mut guard = self.values.borrow_mut();
        let entry = ObservableValue::new(self.element_name(index), value);
        guard.insert(index, entry);
        drop(guard);
        self.structure.report_changed();
    }

    /// Removes and returns the element at the given index.
    pub fn remove(&self, index: usize) -> Option<T>
    where
        T: Clone,
    {
        let mut guard = self.values.borrow_mut();
        if index >= guard.len() {
            return None;
        }
        let value = guard.remove(index).get();
        drop(guard);
        self.structure.report_changed();
        Some(value)
    }

    /// Clears the vector, removing all elements.
    pub fn clear(&self) {
        let mut guard = self.values.borrow_mut();
        if guard.is_empty() {
            return;
        }
        guard.clear();
        drop(guard);
        self.structure.report_changed();
    }

    /// Executes a closure for the mutable element at the specified index.
    pub fn update(&self, index: usize, f: impl FnOnce(&mut T))
    where
        T: Clone,
    {
        if let Some(entry) = self.get_observable(index) {
            entry.update(f);
        }
    }

    /// Returns a snapshot of the vector's contents.
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        let guard = self.values.borrow();
        guard.iter().map(|entry| entry.get()).collect()
    }

    fn element_name(&self, index: usize) -> String {
        format!("{}[{}]", self.name, index)
    }
}

impl<T> fmt::Debug for ObservableVec<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.values.borrow().iter()).finish()
    }
}
