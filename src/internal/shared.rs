//! Shared smart pointer and interior mutability abstractions used across the runtime.
//!
//! The runtime defaults to `Rc`/`RefCell` for single-threaded operation. When
//! the optional `sync` feature is enabled we flip the aliases to `Arc`/`RwLock`
//! so the same code paths become `Send + Sync`.

#[cfg(feature = "sync")]
pub(crate) type Shared<T> = std::sync::Arc<T>;
#[cfg(not(feature = "sync"))]
pub(crate) type Shared<T> = std::rc::Rc<T>;

#[cfg(feature = "sync")]
pub(crate) type SharedWeak<T> = std::sync::Weak<T>;
#[cfg(not(feature = "sync"))]
pub(crate) type SharedWeak<T> = std::rc::Weak<T>;

/// Creates a new reference-counted pointer using the active backend.
pub(crate) fn new_shared<T>(value: T) -> Shared<T> {
    #[cfg(feature = "sync")]
    {
        std::sync::Arc::new(value)
    }

    #[cfg(not(feature = "sync"))]
    {
        std::rc::Rc::new(value)
    }
}

/// Creates a new reference-counted pointer that may hold a self-reference.
pub(crate) fn new_cyclic<T>(f: impl FnOnce(SharedWeak<T>) -> T) -> Shared<T> {
    #[cfg(feature = "sync")]
    {
        std::sync::Arc::new_cyclic(|weak| f(weak.clone()))
    }

    #[cfg(not(feature = "sync"))]
    {
        std::rc::Rc::new_cyclic(|weak| f(weak.clone()))
    }
}

#[cfg(feature = "sync")]
type InnerCell<T> = parking_lot::RwLock<T>;
#[cfg(not(feature = "sync"))]
type InnerCell<T> = std::cell::RefCell<T>;

#[cfg(feature = "sync")]
pub(crate) type SharedReadGuard<'a, T> = parking_lot::RwLockReadGuard<'a, T>;
#[cfg(not(feature = "sync"))]
pub(crate) type SharedReadGuard<'a, T> = std::cell::Ref<'a, T>;

#[cfg(feature = "sync")]
pub(crate) type SharedWriteGuard<'a, T> = parking_lot::RwLockWriteGuard<'a, T>;
#[cfg(not(feature = "sync"))]
pub(crate) type SharedWriteGuard<'a, T> = std::cell::RefMut<'a, T>;

/// Interior mutability wrapper that transparently flips between `RefCell` and `RwLock`.
pub(crate) struct SharedCell<T> {
    inner: InnerCell<T>,
}

impl<T> SharedCell<T> {
    /// Creates a new cell containing the provided value.
    pub(crate) fn new(value: T) -> Self {
        Self {
            inner: InnerCell::new(value),
        }
    }

    /// Returns an immutable borrow of the inner value.
    pub(crate) fn borrow(&self) -> SharedReadGuard<'_, T> {
        #[cfg(feature = "sync")]
        {
            self.inner.read()
        }

        #[cfg(not(feature = "sync"))]
        {
            self.inner.borrow()
        }
    }

    /// Returns a mutable borrow of the inner value.
    pub(crate) fn borrow_mut(&self) -> SharedWriteGuard<'_, T> {
        #[cfg(feature = "sync")]
        {
            self.inner.write()
        }

        #[cfg(not(feature = "sync"))]
        {
            self.inner.borrow_mut()
        }
    }
}

impl<T: Default> Default for SharedCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// Downgrades a strong pointer to a weak reference.
pub(crate) fn downgrade<T: ?Sized>(value: &Shared<T>) -> SharedWeak<T> {
    #[cfg(feature = "sync")]
    {
        std::sync::Arc::downgrade(value)
    }

    #[cfg(not(feature = "sync"))]
    {
        std::rc::Rc::downgrade(value)
    }
}

/// Upgrades a weak pointer back into a strong pointer if possible.
/// Compares whether two strong pointers refer to the same allocation.
#[cfg(test)]
pub(crate) fn ptr_eq<T: ?Sized>(a: &Shared<T>, b: &Shared<T>) -> bool {
    #[cfg(feature = "sync")]
    {
        std::sync::Arc::ptr_eq(a, b)
    }

    #[cfg(not(feature = "sync"))]
    {
        std::rc::Rc::ptr_eq(a, b)
    }
}
