use crate::core::observable::{Atom, ObservableCore};
use crate::internal::shared::{Shared, SharedCell, new_shared};
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;

/// Observable wrapper around `HashSet<K>`.
#[derive(Clone)]
pub struct ObservableSet<K> {
    _name: String,
    structure: Shared<Atom>,
    values: Shared<SharedCell<HashSet<K>>>,
}

impl<K> ObservableSet<K>
where
    K: Eq + Hash + Clone,
{
    /// Creates an empty observable set.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            structure: Atom::new(format!("{name}::structure"), None, None),
            values: new_shared(SharedCell::new(HashSet::new())),
            _name: name,
        }
    }

    /// Returns the number of elements in the set.
    pub fn len(&self) -> usize {
        self.structure.report_observed();
        self.values.borrow().len()
    }

    /// Returns `true` if the set contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the set contains the specified value.
    pub fn contains(&self, value: &K) -> bool {
        self.structure.report_observed();
        self.values.borrow().contains(value)
    }

    /// Inserts a value into the set. Returns `true` if the value was newly inserted.
    pub fn insert(&self, value: K) -> bool {
        let mut guard = self.values.borrow_mut();
        let inserted = guard.insert(value);
        drop(guard);
        if inserted {
            self.structure.report_changed();
        }
        inserted
    }

    /// Removes an element from the set, returning whether the entry was present.
    pub fn remove(&self, value: &K) -> bool {
        let mut guard = self.values.borrow_mut();
        let removed = guard.remove(value);
        drop(guard);
        if removed {
            self.structure.report_changed();
        }
        removed
    }

    /// Clears all elements from the set.
    pub fn clear(&self) {
        let mut guard = self.values.borrow_mut();
        if guard.is_empty() {
            return;
        }
        guard.clear();
        drop(guard);
        self.structure.report_changed();
    }

    /// Returns a snapshot of the set contents.
    pub fn to_vec(&self) -> Vec<K> {
        self.structure.report_observed();
        self.values.borrow().iter().cloned().collect()
    }
}

impl<K> fmt::Debug for ObservableSet<K>
where
    K: Eq + Hash + fmt::Debug + Clone,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self.values.borrow();
        f.debug_set().entries(guard.iter()).finish()
    }
}
