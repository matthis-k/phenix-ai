#[path = "../examples/derived.rs"]
mod example;

use example::*;
use phenix_storage::{Operation, scoped};

#[test]
fn canonical_example() {
    run().unwrap();
}

#[test]
fn independent_observers_chain_diamond_demand_and_b_only_change() {
    let root = TempRoot::new().unwrap();
    let mut demo = build(root.path(), "one").unwrap();
    assert!(
        demo.states
            .iter()
            .all(|s| s.consumed.lock().unwrap().is_empty())
    );
    assert_eq!(demo.drain().unwrap(), 0);
    demo.runtime.start().unwrap();
    demo.write(true, 3).unwrap();
    demo.write(false, 5).unwrap();
    assert!(demo.drain().unwrap() >= 2);
    for state in &demo.states {
        assert!(state.consumed.lock().unwrap().iter().all(|v| *v == 16));
        state.consumed.lock().unwrap().clear();
    }
    demo.write(false, 7).unwrap();
    assert!(demo.drain().unwrap() >= 2);
    for state in &demo.states {
        let values = state.consumed.lock().unwrap();
        assert!(!values.is_empty());
        assert!(values.iter().all(|v| *v == 20));
    }
    for i in 0..2 {
        let before = demo.states[i].consumed.lock().unwrap().len();
        assert_eq!(demo.query_all(i).unwrap(), [10, -4, -40, 20]);
        let values = demo.states[i].consumed.lock().unwrap();
        assert_eq!(values.len(), before + 4);
        assert!(values.iter().all(|v| *v == 20));
    }
    assert_eq!(demo.drain().unwrap(), 0);
}

#[test]
fn fixed_host_grants_reject_missing_read_subscribe_and_write_before_activation() {
    let root = TempRoot::new().unwrap();
    for (plugin, denied) in [
        ("derive", Operation::Read),
        ("derive", Operation::Subscribe),
        ("subscriber", Operation::Subscribe),
        ("writer", Operation::Write),
    ] {
        let result = build_with_policy(root.path(), "one", |policy| {
            for operations in policy.plugin_access.get_mut(plugin).unwrap().values_mut() {
                operations.remove(&denied);
            }
        });
        assert!(matches!(result, Err(ref e) if e == "Denied"));
    }
}

#[test]
fn writer_has_no_read_and_neither_observer_can_write() {
    let root = TempRoot::new().unwrap();
    let mut demo = build(root.path(), "one").unwrap();
    demo.runtime.start().unwrap();
    demo.write(true, 3).unwrap();
    demo.write(false, 5).unwrap();
    assert!(matches!(
        demo.writer.bind(A.read()),
        Err(scoped::Error::Storage(phenix_storage::Error::Denied))
    ));
    for reader in &demo.readers {
        assert!(matches!(
            reader
                .project()
                .unwrap()
                .state::<Amount>("demo.values/a")
                .unwrap()
                .put("value", &Amount(99)),
            Err(phenix_storage::Error::Denied)
        ));
    }
    let values = demo.values().unwrap();
    assert_eq!(values[0].value, Amount(3));
    assert_eq!(values[1].value, Amount(5));
}

#[test]
fn sqlite_reopen_preserves_values_and_revisions_then_advances_only_written_cell() {
    let root = TempRoot::new().unwrap();
    let mut demo = build(root.path(), "one").unwrap();
    demo.runtime.start().unwrap();
    demo.write(true, 3).unwrap();
    demo.write(false, 5).unwrap();
    let saved = demo.values().unwrap();
    drop(demo);

    let mut reopened = build(root.path(), "one").unwrap();
    assert_eq!(reopened.values().unwrap(), saved);
    reopened.runtime.start().unwrap();
    reopened.write(true, 4).unwrap();
    let updated = reopened.values().unwrap();
    assert_eq!(updated[0].value, Amount(4));
    assert_ne!(updated[0].revision, saved[0].revision);
    assert_eq!(updated[1], saved[1]);
    assert_eq!(reopened.query_all(0).unwrap(), [9, -1, -9, 18]);
    drop(reopened);
    assert_eq!(
        build(root.path(), "one").unwrap().values().unwrap(),
        updated
    );
}
