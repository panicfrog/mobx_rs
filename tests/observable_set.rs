use mobx_rs::observable::collections::set::ObservableSet;
use mobx_rs::observable::value::ObservableValue;
use mobx_rs::{autorun, run_pending_reactions};

#[test]
fn test_observable_set_reacts_to_membership_changes() {
    let set = ObservableSet::<String>::new("fruits");
    let contains = ObservableValue::new("contains", false);
    let set_clone = set.clone();
    let contains_clone = contains.clone();

    let handle = autorun(move || {
        contains_clone.set(set_clone.contains(&"apple".to_owned()));
    });

    assert!(!contains.get());

    set.insert("apple".to_owned());
    run_pending_reactions();
    assert!(contains.get());

    set.remove(&"apple".to_owned());
    run_pending_reactions();
    assert!(!contains.get());

    handle.dispose();
}
