#[path = "../examples/derived.rs"]
#[allow(dead_code)]
mod example;

use example::*;
use phenix_graph::{
    compose::NetworkPatch, network::NetworkBuilder, process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, CompositionPolicy, Contribution, Kernel, Key,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{Operation, scoped};
use phenix_storage_graph::derived::{DerivedRuntime, GraphDeriveExt, QuerySpec};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

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

#[test]
fn reader_snapshots_rebuild_and_failed_candidate_does_not_poison_exports() {
    let root = TempRoot::new().unwrap();
    let mut demo = build(root.path(), "one").unwrap();
    demo.runtime.start().unwrap();
    demo.write(true, 0).unwrap();
    demo.write(false, 0).unwrap();
    let mut catalog = PluginCatalog::new();
    let kind = catalog
        .register("derive", |p: reader::Plugin| Ok(p))
        .unwrap();
    let failure_kind = catalog
        .register("failure", |p: FailCandidate| Ok(p))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(
            &kind,
            reader::Plugin {
                name: "derive",
                storage: Arc::new(OnceLock::from(demo.readers[0].clone())),
                state: Arc::default(),
            },
        )
        .unwrap();
    let fail = Arc::new(AtomicBool::new(true));
    plugins
        .load(&failure_kind, FailCandidate(fail.clone()))
        .unwrap();
    let snapshots: Vec<_> = plugins.snapshots().collect();
    // This failure occurs after the reader has registered both candidate artifacts.
    assert!(
        DerivedRuntime::build_with_options(
            "failed",
            snapshots.clone(),
            Arc::new(LocalProcessDispatcher),
            build_options()
        )
        .is_err()
    );
    fail.store(false, Ordering::SeqCst);
    let mut a = DerivedRuntime::build_with_options(
        "a",
        snapshots.clone(),
        Arc::new(LocalProcessDispatcher),
        build_options(),
    )
    .unwrap();
    let mut b = DerivedRuntime::build_with_options(
        "b",
        snapshots,
        Arc::new(LocalProcessDispatcher),
        build_options(),
    )
    .unwrap();
    let key = Key::new("derive", "derive");
    let qa = a.kernel().export::<reader::Queries>(&key).unwrap().clone();
    let qb = b.kernel().export::<reader::Queries>(&key).unwrap().clone();
    assert!(a.bind_query(&qa.sum).is_ok());
    assert!(b.bind_query(&qb.sum).is_ok());
    assert!(a.bind_query(&qb.sum).is_err());
    assert!(b.bind_query(&qa.sum).is_err());
    assert!(a.kernel().export::<u64>(&key).is_none());
    assert!(
        a.kernel()
            .export::<reader::Queries>(&Key::new("missing", "derive"))
            .is_none()
    );
    b.start().unwrap();
    a.start().unwrap();
    assert_eq!(*a.bind_query(&qa.sum).unwrap().query().unwrap(), 0);
    assert_eq!(*b.bind_query(&qb.sum).unwrap().query().unwrap(), 0);
}

struct FailCandidate(Arc<AtomicBool>);
impl PluginInstance for FailCandidate {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let fail = self.0.clone();
        registrar.add(Contribution {
            id: "fail".into(),
            requires: vec![Key::new("derive", "derive")],
            routes: vec![],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                if fail.load(Ordering::SeqCst) {
                    Err("candidate rejected".into())
                } else {
                    Ok(CandidateArtifacts::default())
                }
            })),
        })
    }
}

// Change the composition input between builds of the exact same snapshot. A
// mutable shared activation slot would subscribe A to B's storage in this test.
struct SelectedReader(Arc<Mutex<Arc<scoped::Storage>>>);
impl PluginInstance for SelectedReader {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let selected = self.0.clone();
        registrar.add(Contribution {
            id: "reader".into(),
            requires: vec![],
            routes: vec!["values.refresh".into()],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                let (a, b) = selected.lock().unwrap().bind((A.read(), B.read()))?;
                let mut graph = NetworkBuilder::new();
                let group = graph.sources("values", (&a, &b))?;
                let sum = graph.derive_from("sum", &group, |a: Amount, b: Amount| a.0 + b.0)?;
                let query = graph.query_spec("query", &sum)?;
                let observation = group.observation(16);
                let mut artifacts = CandidateArtifacts {
                    patch: Some(patch(graph.build()?)),
                    ..Default::default()
                };
                artifacts.export(query)?;
                artifacts.activate(move || observation.activate().map_err(Into::into))?;
                Ok(artifacts)
            })),
        })
    }
}

#[test]
fn starting_a_after_b_uses_as_exact_observation() {
    let root = TempRoot::new().unwrap();
    let mut first = build(root.path(), "first").unwrap();
    let mut second = build(root.path(), "second").unwrap();
    first.runtime.start().unwrap();
    second.runtime.start().unwrap();
    for demo in [&first, &second] {
        demo.write(true, 0).unwrap();
        demo.write(false, 0).unwrap();
    }
    let selected = Arc::new(Mutex::new(first.readers[0].clone()));
    let mut catalog = PluginCatalog::new();
    let kind = catalog
        .register("selected", |p: SelectedReader| Ok(p))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(&kind, SelectedReader(selected.clone()))
        .unwrap();
    let snapshots: Vec<_> = plugins.snapshots().collect();
    let mut options = BuildOptions::default();
    use phenix_graph::{
        compose::{ConnectionKey, Footprint},
        network::PortPath,
    };
    let p = |process: &str, port: &str| PortPath {
        process: process.into(),
        port: port.into(),
    };
    options.composition.insert(
        Key::new("selected", "reader"),
        CompositionPolicy {
            grants: [
                Footprint::Component("values".into()),
                Footprint::Component("sum".into()),
                Footprint::Component("query".into()),
                Footprint::InputExport("values.refresh".into()),
                Footprint::OutputExport("query".into()),
                Footprint::Connection(ConnectionKey::new(p("values", "value"), p("sum", "run"))),
                Footprint::Connection(ConnectionKey::new(p("sum", "value"), p("query", "run"))),
            ]
            .into(),
            priority: 0,
        },
    );
    options
        .composition
        .get_mut(&Key::new("selected", "reader"))
        .unwrap()
        .grants
        .extend([
            Footprint::OutputExport("values.snapshot".into()),
            Footprint::OutputExport("values.errors".into()),
        ]);
    let first_options = BuildOptions {
        composition: options.composition.clone(),
        ..Default::default()
    };
    let mut a = DerivedRuntime::build_with_options(
        "a",
        snapshots.clone(),
        Arc::new(LocalProcessDispatcher),
        first_options,
    )
    .unwrap();
    *selected.lock().unwrap() = second.readers[0].clone();
    let mut b = DerivedRuntime::build_with_options(
        "b",
        snapshots,
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .unwrap();
    b.start().unwrap();
    a.start().unwrap();
    for runtime in [&mut a, &mut b] {
        assert_eq!(runtime.drain(64).len(), 1); // Bootstrap only.
        runtime.kernel().run_steps(128).unwrap();
    }
    first.write(true, 7).unwrap();
    let events = a.drain(64);
    assert!(!events.is_empty());
    assert!(events.into_iter().all(|event| event.result.is_ok()));
    assert!(b.drain(64).is_empty());
    second.write(false, 11).unwrap();
    assert!(a.drain(64).is_empty());
    let events = b.drain(64);
    assert!(!events.is_empty());
    assert!(events.into_iter().all(|event| event.result.is_ok()));
    for (runtime, expected) in [(&a, 7), (&b, 11)] {
        let query = runtime
            .kernel()
            .export::<QuerySpec<i64>>(&Key::new("selected", "reader"))
            .unwrap();
        assert_eq!(
            *runtime.bind_query(query).unwrap().query().unwrap(),
            expected
        );
    }
}

struct CompetingFactories;
impl PluginInstance for CompetingFactories {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        registrar.add(Contribution {
            id: "competing".into(),
            requires: vec![],
            routes: vec![],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(|_| {
                let mut artifacts = CandidateArtifacts::default();
                artifacts.activate(|| panic!("must not activate"))?;
                artifacts.activate(|| panic!("must not activate"))?;
                Ok(artifacts)
            })),
        })
    }
}

#[test]
fn duplicate_candidate_factories_are_rejected_not_silently_overridden() {
    let mut catalog = PluginCatalog::new();
    let kind = catalog
        .register("competing", |p: CompetingFactories| Ok(p))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, CompetingFactories).unwrap();
    let error = Kernel::build(
        "competing",
        plugins.snapshots(),
        Arc::new(LocalProcessDispatcher),
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("candidate activation already registered"),
        "{error}"
    );
}
