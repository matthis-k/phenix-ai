#[allow(dead_code)]
#[path = "../examples/derived.rs"]
mod example;
use phenix_graph::{
    compose::NetworkPatch, network::NetworkBuilder, process::LocalProcessDispatcher,
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, CompositionPolicy, Contribution, Kernel, Key, Producer,
    WakeHandle,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{Address, Grant, Notice, Operation, Registry, Schema, Scope, Session};
use phenix_storage_graph::StorageBinding;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

fn storage() -> (Registry, Session, Address) {
    let registry = Registry::new();
    registry.mount_sqlite("main", ":memory:").unwrap();
    let address = Address::parse("store://main/items").unwrap();
    registry
        .register(
            &address,
            Schema {
                name: "items".into(),
                revision: 1,
                structure: "i64".into(),
                codec: "test".into(),
            },
        )
        .unwrap();
    let session = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::Subscribe, Operation::SqlAdmin],
    )]);
    session
        .transaction(&address, |sql| {
            sql.execute("CREATE TABLE items (value INTEGER)", &[])?;
            Ok(())
        })
        .unwrap();
    (registry, session, address)
}

fn write(session: &Session, address: &Address) {
    session
        .transaction(address, |sql| {
            sql.execute("INSERT INTO items VALUES (1)", &[])?;
            Ok(())
        })
        .unwrap();
}

struct Observer {
    route: String,
    session: Session,
    observed: Arc<Mutex<Vec<Notice>>>,
}
impl PluginInstance for Observer {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let route = self.route.clone();
        let target = route.clone();
        let session = self.session.clone();
        let observed = self.observed.clone();
        registrar.add(Contribution {
            id: "observer".into(),
            requires: vec![],
            routes: vec![route.clone()],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                let mut graph = NetworkBuilder::new();
                let observed = observed.clone();
                let sink = example::sink(&mut graph, &route, move |notice: Notice| {
                    observed.lock().unwrap().push(notice);
                })?;
                graph.expose_input(
                    &route,
                    sink.input::<Notice>("run")?,
                    std::num::NonZeroUsize::new(16).unwrap(),
                )?;
                let session = session.clone();
                let target = target.clone();
                let mut artifacts = CandidateArtifacts {
                    patch: Some(example::patch(graph.build()?)),
                    ..Default::default()
                };
                artifacts.activate(move || {
                    let subscription = session.subscribe(Scope::Mount("main".into()), 2)?;
                    Ok(Box::new(StorageBinding::new(subscription, target.clone())))
                })?;
                Ok(artifacts)
            })),
        })
    }
}

fn options(routes: &[(&str, &str)]) -> BuildOptions {
    use phenix_graph::compose::Footprint;
    let mut options = BuildOptions::default();
    for (owner, route) in routes {
        options.composition.insert(
            Key::new(*owner, "observer"),
            CompositionPolicy {
                grants: [
                    Footprint::Component((*route).into()),
                    Footprint::InputExport((*route).into()),
                ]
                .into(),
                priority: 0,
            },
        );
    }
    options
}

#[test]
fn independent_plugins_preserve_tails_and_overflow_per_ingress() {
    let (_registry, session, address) = storage();
    let mut catalog = PluginCatalog::new();
    let a = catalog
        .register("a", |plugin: Observer| Ok(plugin))
        .unwrap();
    let b = catalog
        .register("b", |plugin: Observer| Ok(plugin))
        .unwrap();
    let logs = [
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
    ];
    let mut plugins = catalog.instance();
    for (handle, route, observed) in [(&a, "a", &logs[0]), (&b, "b", &logs[1])] {
        plugins
            .load(
                handle,
                Observer {
                    route: route.into(),
                    session: session.clone(),
                    observed: observed.clone(),
                },
            )
            .unwrap();
    }
    let mut kernel = Kernel::build_with_options(
        "bindings",
        plugins.snapshots(),
        Arc::new(LocalProcessDispatcher),
        options(&[("a", "a"), ("b", "b")]),
    )
    .unwrap();
    kernel.start().unwrap();
    write(&session, &address);
    write(&session, &address);
    kernel.drain(1).remove(0).result.unwrap();
    kernel.run_steps(128).unwrap();
    assert_eq!(logs[0].lock().unwrap().len(), 1);
    assert!(logs[1].lock().unwrap().is_empty());
    // A has a buffered tail; B's undrained subscription overflows independently.
    write(&session, &address);
    write(&session, &address);
    let events = kernel.drain(3);
    assert_eq!(events.len(), 2);
    assert!(events.into_iter().all(|e| e.result.is_ok()));
    kernel.run_steps(128).unwrap();
    assert_eq!(logs[0].lock().unwrap().len(), 2);
    assert!(matches!(
        logs[1].lock().unwrap().as_slice(),
        [Notice::Overflow { through: 5 }]
    ));
    assert_eq!(kernel.drain(4).len(), 2);
    kernel.run_steps(128).unwrap();
    assert_eq!(logs[0].lock().unwrap().len(), 4);
    write(&session, &address);
    assert_eq!(kernel.drain(2).len(), 2);
    kernel.run_steps(128).unwrap();
    assert_eq!(logs[0].lock().unwrap().len(), 5);
    assert_eq!(logs[1].lock().unwrap().len(), 2);
}

#[test]
fn blocking_driver_wakes_for_storage_publication() {
    let (_registry, session, address) = storage();
    let mut catalog = PluginCatalog::new();
    let observer = catalog
        .register("observer", |plugin: Observer| Ok(plugin))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(
            &observer,
            Observer {
                route: "changed".into(),
                session: session.clone(),
                observed: Arc::default(),
            },
        )
        .unwrap();
    let mut kernel = Kernel::build_with_options(
        "wake",
        plugins.snapshots(),
        Arc::new(LocalProcessDispatcher),
        options(&[("observer", "changed")]),
    )
    .unwrap();
    kernel.start().unwrap();
    let wake = WakeHandle::default();
    let stop = wake.clone();
    let (tx, rx) = mpsc::channel();
    let driver = std::thread::spawn(move || {
        kernel.run_until(&wake, |event| {
            tx.send(event.result.is_ok()).unwrap();
        })
    });
    write(&session, &address);
    let result = rx.recv_timeout(Duration::from_secs(5));
    stop.shutdown();
    driver.join().unwrap().unwrap();
    assert!(result.unwrap());
}

#[test]
fn closed_subscription_is_a_poll_error() {
    let (registry, session, _address) = storage();
    let subscription = session.subscribe(Scope::Mount("main".into()), 1).unwrap();
    let mut binding = StorageBinding::new(subscription, "changed");
    drop(session);
    drop(registry);
    assert!(matches!(
        binding
            .poll()
            .err()
            .unwrap()
            .downcast_ref::<phenix_storage::Error>(),
        Some(phenix_storage::Error::SubscriptionClosed)
    ));
}

#[test]
fn subscription_waker_covers_ready_overflow_and_replacement() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Wake;
    #[derive(Default)]
    struct Counter(AtomicUsize);
    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (_registry, session, address) = storage();
    let mut subscription = session.subscribe(Scope::Mount("main".into()), 1).unwrap();
    let first = Arc::new(Counter::default());
    subscription.set_waker(first.clone().into()).unwrap();
    assert_eq!(first.0.load(Ordering::SeqCst), 0);
    write(&session, &address);
    assert_eq!(first.0.load(Ordering::SeqCst), 1);
    let second = Arc::new(Counter::default());
    subscription.set_waker(second.clone().into()).unwrap();
    assert_eq!(second.0.load(Ordering::SeqCst), 1);
    write(&session, &address);
    assert_eq!(first.0.load(Ordering::SeqCst), 1);
    assert_eq!(second.0.load(Ordering::SeqCst), 2);
    subscription.set_waker(second.clone().into()).unwrap();
    assert_eq!(second.0.load(Ordering::SeqCst), 3);
    assert!(matches!(
        subscription.drain().unwrap().as_slice(),
        [Notice::Overflow { .. }]
    ));
    write(&session, &address);
    assert!(matches!(
        subscription.drain().unwrap().as_slice(),
        [Notice::Changed(_)]
    ));
}
