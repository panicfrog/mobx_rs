use crate::core::observable::{Atom, ObservableCore};
use crate::internal::shared::{Shared, SharedCell, new_shared};
use crate::observable::value::ObservableValue;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

/// Observable analog of `HashMap<K, V>` with fine-grained key/value tracking.
#[derive(Clone)]
pub struct ObservableMap<K, V> {
    name: String,
    structure: Shared<Atom>,
    entries: Shared<SharedCell<HashMap<K, ObservableValue<V>>>>,
}

impl<K, V> ObservableMap<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Creates an empty observable map.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            structure: Atom::new(format!("{name}::structure"), None, None),
            entries: new_shared(SharedCell::new(HashMap::new())),
            name,
        }
    }

    /// Returns the number of key-value pairs stored in the map.
    pub fn len(&self) -> usize {
        self.structure.report_observed();
        self.entries.borrow().len()
    }

    /// Returns `true` if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the map contains the specified key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.structure.report_observed();
        self.entries.borrow().contains_key(key)
    }

    /// Retrieves a cloned value for the specified key.
    pub fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.structure.report_observed();
        self.entries.borrow().get(key).map(|value| value.get())
    }

    /// Provides access to the observable value for the specified key.
    pub fn get_observable(&self, key: &K) -> Option<ObservableValue<V>> {
        self.entries.borrow().get(key).cloned()
    }

    /// Inserts a key-value pair, returning the previous value if present.
    pub fn insert(&self, key: K, value: V) -> Option<V>
    where
        V: Clone,
    {
        let entry_name = self.entry_name(&key);
        let mut guard = self.entries.borrow_mut();
        let result = if let Some(existing) = guard.get(&key) {
            let observable = existing.clone();
            let previous = observable.get();
            drop(guard);
            observable.set(value);
            Some(previous)
        } else {
            guard.insert(key, ObservableValue::new(entry_name, value));
            drop(guard);
            None
        };
        self.structure.report_changed();
        result
    }

    /// Removes a key-value pair, returning the stored value if it existed.
    pub fn remove(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        let mut guard = self.entries.borrow_mut();
        let result = guard.remove(key).map(|value| value.get());
        drop(guard);
        if result.is_some() {
            self.structure.report_changed();
        }
        result
    }

    /// Clears all entries from the map.
    pub fn clear(&self) {
        let mut guard = self.entries.borrow_mut();
        if guard.is_empty() {
            return;
        }
        guard.clear();
        drop(guard);
        self.structure.report_changed();
    }

    /// Returns a snapshot of the map contents as key-value pairs.
    pub fn to_vec(&self) -> Vec<(K, V)>
    where
        V: Clone,
    {
        self.structure.report_observed();
        self.entries
            .borrow()
            .iter()
            .map(|(key, value)| (key.clone(), value.get()))
            .collect()
    }

    fn entry_name(&self, _key: &K) -> String {
        format!("{}::entry", self.name)
    }
}

impl<K, V> fmt::Debug for ObservableMap<K, V>
where
    K: Eq + Hash + fmt::Debug + Clone,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self.entries.borrow();
        let mut map = f.debug_map();
        for (key, value) in guard.iter() {
            map.entry(key, &value);
        }
        map.finish()
    }
}
