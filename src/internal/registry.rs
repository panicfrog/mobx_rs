#![allow(dead_code)]

use crate::internal::shared::{Shared, SharedWeak, downgrade};
use std::collections::HashMap;
use std::hash::Hash;
use std::num::NonZeroU64;

/// Maintains a mapping between strongly-held runtime objects and their
/// lightweight identifier references.
///
/// The registry issues monotonically increasing, non-zero identifiers and keeps
/// only weak references to the registered objects. This allows the runtime to
/// look up objects by ID without preventing them from being dropped once all
/// strong references are gone.
pub(crate) struct IdRegistry<Id, T: ?Sized> {
    last_issued: u64,
    entries: HashMap<Id, SharedWeak<T>>,
}

impl<Id, T: ?Sized> Default for IdRegistry<Id, T>
where
    Id: From<NonZeroU64> + Into<NonZeroU64> + Copy + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Id, T: ?Sized> IdRegistry<Id, T>
where
    Id: From<NonZeroU64> + Into<NonZeroU64> + Copy + Eq + Hash,
{
    /// Creates an empty registry with no issued identifiers.
    pub(crate) fn new() -> Self {
        Self {
            last_issued: 0,
            entries: HashMap::new(),
        }
    }

    /// Issues the next unique identifier without registering an object.
    ///
    /// Callers that need to construct their value before registration can
    /// reserve an ID upfront and later associate it with [`attach`].
    pub(crate) fn reserve(&mut self) -> Id {
        self.last_issued = self
            .last_issued
            .checked_add(1)
            .expect("ID allocator overflowed u64");
        let raw = NonZeroU64::new(self.last_issued).expect("reserved ID must be non-zero");
        Id::from(raw)
    }

    /// Registers a strong pointer and returns the freshly assigned identifier.
    pub(crate) fn register(&mut self, value: &Shared<T>) -> Id {
        let id = self.reserve();
        self.attach(id, value);
        id
    }

    /// Associates an existing identifier with the provided strong pointer.
    pub(crate) fn attach(&mut self, id: Id, value: &Shared<T>) {
        self.entries.insert(id, downgrade(value));
    }

    /// Attempts to upgrade the weak handle associated with the ID.
    pub(crate) fn get(&self, id: Id) -> Option<Shared<T>> {
        self.entries.get(&id).and_then(SharedWeak::upgrade)
    }

    /// Removes the entry for the provided identifier.
    ///
    /// Returns `true` when an entry was present and removed.
    pub(crate) fn remove(&mut self, id: Id) -> bool {
        self.entries.remove(&id).is_some()
    }

    /// Drops all weak handles that no longer have a corresponding strong owner.
    ///
    /// Returns the number of stale entries removed.
    pub(crate) fn purge_stale(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, weak| weak.strong_count() > 0);
        before - self.entries.len()
    }

    /// Returns the number of currently tracked IDs.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the registry contains no entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::ids::ObservableId;
    use crate::internal::shared::{new_shared, ptr_eq};

    #[test]
    fn test_register_assigns_unique_ids() {
        let mut registry: IdRegistry<ObservableId, usize> = IdRegistry::new();
        let value_a = new_shared(1usize);
        let value_b = new_shared(2usize);

        let id_a = registry.register(&value_a);
        let id_b = registry.register(&value_b);

        assert_ne!(u64::from(id_a), u64::from(id_b));
        assert_eq!(registry.len(), 2);
        assert!(ptr_eq(&registry.get(id_a).unwrap(), &value_a));
        assert!(ptr_eq(&registry.get(id_b).unwrap(), &value_b));
    }

    #[test]
    fn test_purge_removes_dropped_entries() {
        let mut registry: IdRegistry<ObservableId, usize> = IdRegistry::new();
        let id_a;
        {
            let value_a = new_shared(1usize);
            id_a = registry.register(&value_a);
            assert!(registry.get(id_a).is_some());
        }
        // Dropping value_a should leave only a stale weak reference.
        assert!(registry.get(id_a).is_none());
        assert_eq!(registry.purge_stale(), 1);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_reserve_then_attach() {
        let mut registry: IdRegistry<ObservableId, usize> = IdRegistry::new();
        let id = registry.reserve();
        let value = new_shared(7usize);
        registry.attach(id, &value);

        assert_eq!(registry.len(), 1);
        assert!(ptr_eq(&registry.get(id).unwrap(), &value));
    }
}
