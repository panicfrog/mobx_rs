use mobx_rs::observable::collections::vec::ObservableVec;
use mobx_rs::observable::value::ObservableValue;
use mobx_rs::{autorun, run_pending_reactions};
#[cfg(feature = "sync")]
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "sync")]
fn with_serialized_runtime(f: impl FnOnce()) {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = RUNTIME_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().expect("runtime lock poisoned");
    f();
}

#[cfg(not(feature = "sync"))]
fn with_serialized_runtime(f: impl FnOnce()) {
    f();
}

#[test]
fn test_observable_vec_tracks_length_changes() {
    with_serialized_runtime(|| {
        let vec = ObservableVec::new("numbers");
        let last_len = ObservableValue::new("len", 0usize);
        let vec_clone = vec.clone();
        let len_clone = last_len.clone();

        let handle = autorun(move || {
            let current = vec_clone.len();
            len_clone.set(current);
        });

        assert_eq!(last_len.get(), 0);

        vec.push(1);
        run_pending_reactions();
        assert_eq!(last_len.get(), 1);

        vec.pop();
        run_pending_reactions();
        assert_eq!(last_len.get(), 0);

        handle.dispose();
    });
}

#[test]
fn test_observable_vec_element_updates_trigger_reaction() {
    with_serialized_runtime(|| {
        let vec = ObservableVec::new("numbers");
        vec.push(10);
        let first = ObservableValue::new("first", 0);
        let vec_clone = vec.clone();
        let first_clone = first.clone();

        let handle = autorun(move || {
            if let Some(value) = vec_clone.get(0) {
                first_clone.set(value);
            }
        });

        assert_eq!(first.get(), 10);

        vec.set(0, 20).unwrap();
        run_pending_reactions();
        assert_eq!(first.get(), 20);

        handle.dispose();
    });
}
