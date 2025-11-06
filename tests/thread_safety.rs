#![cfg(feature = "sync")]

use mobx_rs::observable::value::ObservableValue;
use mobx_rs::{
    ActionPolicy, autorun, drain_strict_mode_warnings, run_pending_reactions, set_enforce_actions,
};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

fn runtime_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn test_reaction_runs_on_triggering_thread() {
    let _guard = runtime_test_lock().lock().expect("lock poisoned");
    set_enforce_actions(ActionPolicy::Never);
    drain_strict_mode_warnings();
    let counter = ObservableValue::new("threaded_counter", 0);
    let executions = Arc::new(Mutex::new(Vec::new()));
    let executions_clone = Arc::clone(&executions);

    let reaction_handle = autorun({
        let counter = counter.clone();
        move || {
            let value = counter.get();
            let thread_id = thread::current().id();
            executions_clone
                .lock()
                .expect("record mutex poisoned")
                .push((thread_id, value));
        }
    });

    // Initial run happens on the main thread.
    assert_eq!(executions.lock().expect("record mutex poisoned").len(), 1);

    let worker_thread_id = thread::spawn({
        let counter = counter.clone();
        move || {
            counter.set(1);
            thread::current().id()
        }
    })
    .join()
    .expect("worker thread panicked");

    let log = executions.lock().expect("record mutex poisoned");
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].0, worker_thread_id);
    assert_eq!(log[1].1, 1);
    drop(log);

    reaction_handle.dispose();
    set_enforce_actions(ActionPolicy::Never);
}

#[test]
fn test_strict_mode_violation_across_threads() {
    let _guard = runtime_test_lock().lock().expect("lock poisoned");
    set_enforce_actions(ActionPolicy::Always);
    drain_strict_mode_warnings();

    let counter = ObservableValue::new("strict_counter", 0);

    thread::spawn({
        let counter = counter.clone();
        move || {
            counter.set(5);
        }
    })
    .join()
    .expect("worker thread panicked");

    let warnings = drain_strict_mode_warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("strict_counter"));

    set_enforce_actions(ActionPolicy::Never);
}

#[test]
fn test_pending_reactions_can_flush_after_cross_thread_change() {
    let _guard = runtime_test_lock().lock().expect("lock poisoned");
    set_enforce_actions(ActionPolicy::Never);
    drain_strict_mode_warnings();

    set_enforce_actions(ActionPolicy::Never);
    let counter = ObservableValue::new("flush_counter", 0);
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_clone = Arc::clone(&observed);

    let reaction_handle = autorun({
        let counter = counter.clone();
        move || {
            observed_clone
                .lock()
                .expect("record mutex poisoned")
                .push(counter.get());
        }
    });

    assert_eq!(observed.lock().expect("record mutex poisoned").len(), 1);

    thread::spawn({
        let counter = counter.clone();
        move || {
            counter.set(2);
        }
    })
    .join()
    .expect("worker thread panicked");

    // Inline scheduler should have flushed automatically, but double-check by forcing a drain.
    run_pending_reactions();

    let record = observed.lock().expect("record mutex poisoned");
    assert_eq!(record.len(), 2);
    assert_eq!(record[1], 2);
    drop(record);

    reaction_handle.dispose();
    set_enforce_actions(ActionPolicy::Never);
}
