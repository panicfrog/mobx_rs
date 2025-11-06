use mobx_rs::observable::value::ObservableValue;
use mobx_rs::{
    ActionPolicy, Computed, ComputedOptions, action, autorun, run_pending_reactions,
    set_enforce_actions,
};
use std::sync::{Arc, Mutex};

#[test]
fn test_observable_value_integrates_with_computed_and_reaction() {
    set_enforce_actions(ActionPolicy::Always);
    let counter = ObservableValue::new("counter", 0);

    let doubled = {
        let counter = counter.clone();
        Computed::new(ComputedOptions::new(move || counter.get() * 2))
    };

    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_clone = observed.clone();

    let handle = autorun(move || {
        observed_clone
            .lock()
            .expect("mutex poisoned")
            .push(doubled.get());
    });

    action("increment", || counter.update(|value| *value += 1));
    run_pending_reactions();

    action("set", || counter.set(10));
    run_pending_reactions();

    handle.dispose();
    set_enforce_actions(ActionPolicy::Never);

    let observed_guard = observed.lock().expect("mutex poisoned");
    assert!(!observed_guard.is_empty());
    assert_eq!(observed_guard.first(), Some(&0));
    assert!(observed_guard.iter().any(|&value| value == 2));
    assert_eq!(observed_guard.last(), Some(&20));
}
