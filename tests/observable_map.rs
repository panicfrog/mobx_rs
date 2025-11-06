use mobx_rs::observable::collections::map::ObservableMap;
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
fn test_observable_map_reaction_on_value_changes() {
    with_serialized_runtime(|| {
        let map = ObservableMap::<String, i32>::new("scores");
        map.insert("alice".to_owned(), 1);
        run_pending_reactions();

        let snapshot = ObservableValue::new("value", None);
        let map_clone = map.clone();
        let snapshot_clone = snapshot.clone();

        let handle = autorun(move || {
            snapshot_clone.set(map_clone.get(&"alice".to_owned()));
        });

        assert_eq!(snapshot.get(), Some(1));

        map.insert("alice".to_owned(), 7);
        run_pending_reactions();
        println!("snapshot after insert: {:?}", map.to_vec());
        assert_eq!(map.get(&"alice".to_owned()), Some(7));
        assert_eq!(snapshot.get(), Some(7));

        map.remove(&"alice".to_owned());
        run_pending_reactions();
        assert_eq!(snapshot.get(), None);

        handle.dispose();
    });
}

#[test]
fn test_observable_map_len_tracks_structure() {
    with_serialized_runtime(|| {
        let map = ObservableMap::<String, i32>::new("scores");
        let len = ObservableValue::new("len", 0usize);
        let map_clone = map.clone();
        let len_clone = len.clone();

        let handle = autorun(move || {
            len_clone.set(map_clone.len());
        });

        assert_eq!(len.get(), 0);

        map.insert("bob".to_owned(), 1);
        run_pending_reactions();
        assert_eq!(len.get(), 1);

        map.clear();
        run_pending_reactions();
        assert_eq!(len.get(), 0);

        handle.dispose();
    });
}
