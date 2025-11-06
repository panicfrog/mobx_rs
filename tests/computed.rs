#![cfg(not(feature = "sync"))]

use mobx_rs::{Computed, ComputedOptions, ComputedSetError};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_computed_public_api_can_get_and_set() {
    let backing = Rc::new(RefCell::new(0));
    let backing_for_get = Rc::clone(&backing);
    let backing_for_set = Rc::clone(&backing);
    let computed = Computed::new(
        ComputedOptions::new(move || *backing_for_get.borrow() * 2)
            .name("double")
            .setter(move |value| *backing_for_set.borrow_mut() = value),
    );

    assert_eq!(computed.get(), 0);
    computed.set(21).unwrap();
    assert_eq!(*backing.borrow(), 21);
}

#[test]
fn test_computed_without_setter_returns_error() {
    let computed = Computed::new(ComputedOptions::new(|| 5));
    let err = computed.set(10).expect_err("expected setter error");
    assert!(matches!(err, ComputedSetError { .. }));
}
