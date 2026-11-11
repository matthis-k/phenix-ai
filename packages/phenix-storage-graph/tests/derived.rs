mod support;
use phenix_graph::{
    Value,
    network::NetworkBuilder,
    process::{ActivationJob, DispatchResult, LocalProcessDispatcher, ProcessDispatcher},
    runtime::{RequestStatus, SessionState},
};
use phenix_storage::{
    Backend, Codec, Grant, MemoryBackend, Operation, Revisioned, Scope, TypedKv,
    scoped::{Identity, ResourceCatalog},
};
use phenix_storage_graph::derived::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use support::*;

struct ReadProbe {
    memory: MemoryBackend,
    denied: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
}
impl Backend for ReadProbe {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(self)
    }
}
impl TypedKv for ReadProbe {
    fn supports_atomic_read_many(&self) -> bool {
        true
    }
    fn read_many(
        &mut self,
        keys: &[phenix_storage::snapshot::ReadKey<'_>],
    ) -> phenix_storage::Result<Vec<Option<Revisioned<Vec<u8>>>>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.denied.load(Ordering::SeqCst) {
            Err(phenix_storage::Error::Denied)
        } else {
            self.memory.read_many(keys)
        }
    }
    fn get(&mut self, d: &str, k: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
        self.memory.get(d, k)
    }
    fn put(&mut self, d: &str, k: &str, v: Vec<u8>) -> phenix_storage::Result<()> {
        self.memory.put(d, k, v)
    }
    fn delete(&mut self, d: &str, k: &str) -> phenix_storage::Result<bool> {
        self.memory.delete(d, k)
    }
}

#[test]
fn one_atomic_batch_per_query_no_build_or_binding_reads_and_denial_recovers() {
    let reads = Arc::new(AtomicUsize::new(0));
    let denied = Arc::new(AtomicBool::new(false));
    let f = Fixture::with_backend(
        false,
        ReadProbe {
            memory: MemoryBackend::default(),
            denied: denied.clone(),
            reads: reads.clone(),
        },
    );
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let mut runtime = local(plugin);
    runtime.bind_query(&spec).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(matches!(
        runtime.bind_query(&spec).unwrap().query(),
        Err(Error::Graph(_))
    ));
    runtime.start().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert_eq!(*query.query().unwrap(), 8);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    denied.store(true, Ordering::SeqCst);
    assert!(
        matches!(query.query(),Err(Error::Wave(e)) if e.join(" ").contains("source[Storage]: Denied"))
    );
    denied.store(false, Ordering::SeqCst);
    f.put(&f.a, 4);
    assert_eq!(*query.query().unwrap(), 9);
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    f.session.delete(f.a.source().address(), "value").unwrap();
    assert!(
        matches!(query.query(),Err(Error::Wave(e)) if e.join(" ").contains("source[MissingInput]"))
    );
    f.put(&f.a, 10);
    assert_eq!(*query.query().unwrap(), 15);
}

#[test]
fn source_is_one_native_process_with_one_snapshot_and_terminal_capture() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let mut runtime = local(plugin);
    let compiled = runtime.kernel().compiled();
    assert_eq!(compiled.processes().len(), 3);
    assert!(
        compiled
            .inspect("sum.sources")
            .unwrap()
            .interface
            .output("value")
            .is_some()
    );
    runtime.start().unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
}

#[test]
fn optional_missing_and_codec_failures_recover_without_result_payloads() {
    let f = Fixture::new(false);
    f.put(&f.b, 5);
    let mut graph = NetworkBuilder::new();
    let optional = f.a.optional();
    let sum = graph
        .derive(
            "optional",
            (&optional, &f.b),
            |a: Option<Number>, b: Number| a.map_or(0, |a| a.0) + b.0,
        )
        .unwrap();
    let spec = graph.query_spec("query", &sum).unwrap();
    let mut runtime = local(authored(graph, spec.clone(), None));
    runtime.start().unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 5);
    struct Bad;
    impl Codec for Bad {
        type Value = Number;
        fn schema(&self) -> phenix_storage::Schema {
            Format.schema()
        }
        fn encode(&self, _: &Number) -> phenix_storage::Result<Vec<u8>> {
            Ok(vec![0])
        }
        fn decode(&self, _: &[u8]) -> phenix_storage::Result<Number> {
            unreachable!()
        }
    }
    f.session
        .put(f.b.source().address(), "value", &Number(0), &Bad)
        .unwrap();
    assert!(
        matches!(runtime.bind_query(&spec).unwrap().query(),Err(Error::Wave(e)) if e.join(" ").contains("Codec"))
    );
    f.put(&f.b, 9);
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 9);
    f.put(&f.a, 2);
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 11);
}

#[test]
fn shared_siblings_chain_diamond_all_arities_and_ordinary_consumer() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let mut g = NetworkBuilder::new();
    let group = g.sources("values", (&f.a, &f.b)).unwrap();
    let sum = g
        .derive_from("sum", &group, |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let diff = g
        .derive_from("diff", &group, |a: Number, b: Number| a.0 - b.0)
        .unwrap();
    let twice = g.derive("twice", (&sum,), |s| s * 2).unwrap();
    let both = g.derive("both", (&sum, &diff), |s, d| s + d).unwrap();
    let diamond = g.derive("diamond", (&sum, &twice), |s, t| s + t).unwrap();
    let three = g
        .derive("three", (&sum, &diff, &twice), |s, d, t| s + d + t)
        .unwrap();
    let four = g
        .derive("four", (&sum, &diff, &twice, &both), |s, d, t, b| {
            s + d + t + b
        })
        .unwrap();
    let s3 = g
        .derive(
            "stored3",
            (&f.a, &f.b, &f.a),
            |a: Number, b: Number, c: Number| a.0 + b.0 + c.0,
        )
        .unwrap();
    let s4 = g
        .derive(
            "stored4",
            (&f.a, &f.b, &f.a, &f.b),
            |a: Number, b: Number, c: Number, d: Number| a.0 + b.0 + c.0 + d.0,
        )
        .unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    let consumed = log.clone();
    let consumer = sink(&mut g, "consumer", move |v: i64| {
        consumed.lock().unwrap().push(v)
    })
    .unwrap();
    g.connect(sum.output(), consumer.input("run").unwrap(), capacity())
        .unwrap();
    let specs: Vec<_> = [
        (&sum, "sum.q"),
        (&both, "both.q"),
        (&diamond, "diamond.q"),
        (&three, "three.q"),
        (&four, "four.q"),
        (&s3, "s3.q"),
        (&s4, "s4.q"),
    ]
    .into_iter()
    .map(|(v, n)| g.query_spec(n, v).unwrap())
    .collect();
    let mut runtime = local(authored(g, specs[0].clone(), None));
    runtime.start().unwrap();
    for (spec, expected) in specs.iter().zip([8, 6, 24, 22, 28, 11, 16]) {
        assert_eq!(
            *runtime.bind_query(spec).unwrap().query().unwrap(),
            expected
        );
    }
    assert_eq!(*log.lock().unwrap(), vec![8; 5]);
}

#[test]
fn mixed_sources_roots_foreign_handles_and_name_collisions_reject_before_mutation() {
    let f = Fixture::new(false);
    let foreign = Fixture::new(false);
    let mut g = NetworkBuilder::new();
    assert!(matches!(
        g.sources("group", (&f.a, &foreign.b)),
        Err(Error::Storage(phenix_storage::Error::MixedSnapshotSources))
    ));
    let group = g.sources("group", (&f.a, &f.b)).unwrap();
    let sum = g
        .derive_from("sum", &group, |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let other = g.derive("other", (&f.a,), |a: Number| a.0).unwrap();
    assert!(matches!(
        g.derive("bad", (&sum, &other), |a, b| a + b),
        Err(Error::MultipleRefreshRoots)
    ));
    g.derive("bad", (&sum,), |a| a).unwrap();
    assert!(g.derive("sum", (&f.a,), |a: Number| a.0).is_err());
    g.sources("sum.sources", (&f.a,)).unwrap();
    let mut second = NetworkBuilder::new();
    assert!(second.derive("foreign", (&sum,), |a| a).is_err());
    second.sources("foreign", (&f.a,)).unwrap();
    assert!(second.query_spec("tap", &sum).is_err());
    second.component("foreign").unwrap();
    assert!(g.inspect(&g.component("sum").unwrap()).is_ok());
}

#[test]
fn same_mount_grouping_does_not_union_dataset_session_grants() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let identity = Identity::user("u").unwrap().project("p").unwrap();
    let mut catalog = ResourceCatalog::new();
    for (spec, cell) in [(A, &f.a), (B, &f.b)] {
        catalog
            .bind(
                spec.resource_spec().resolve(&identity).unwrap(),
                cell.source().address().clone(),
            )
            .unwrap();
    }
    let bindings = catalog.freeze();
    let only_a = bindings.access(
        f.registry.session([Grant::new(
            Scope::Dataset(f.a.source().address().clone()),
            [Operation::Read, Operation::Subscribe],
        )]),
        identity.clone(),
    );
    let only_b = bindings.access(
        f.registry.session([Grant::new(
            Scope::Dataset(f.b.source().address().clone()),
            [Operation::Read],
        )]),
        identity,
    );
    let a = only_a.bind(A.read()).unwrap();
    let b = only_b.bind(B.read()).unwrap();
    assert!(only_a.bind(B.read()).is_err());
    assert!(only_b.bind(A.read()).is_err());
    let mut g = NetworkBuilder::new();
    let group = g.sources("group", (&a, &b)).unwrap();
    for _ in 0..2 {
        assert!(matches!(
            group.observation(1).activate(),
            Err(Error::Storage(phenix_storage::Error::Denied))
        ));
    }
    let sum = g
        .derive_from("sum", &group, |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let spec = g.query_spec("query", &sum).unwrap();
    let mut runtime = local(authored(g, spec.clone(), None));
    runtime.start().unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
    f.put(&f.b, 7);
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 10);
    assert!(only_a.bind(B.read()).is_err());
    assert!(only_b.bind(A.read()).is_err());
}

#[test]
fn observation_subscribes_before_bootstrap_preserves_overflow_and_round_robin() {
    let f = Fixture::new(true);
    let mut g = NetworkBuilder::new();
    let group = g.sources("values", (&f.a, &f.b)).unwrap();
    let mut producer = group.observation(1).activate().unwrap();
    f.put(&f.b, 6);
    let event = producer.poll().unwrap().unwrap();
    assert_eq!(event.input, "values.refresh");
    assert!(matches!(
        event.value.borrow::<RefreshCause>(),
        Some(RefreshCause::Resynchronize)
    ));
    assert!(matches!(
        producer
            .poll()
            .unwrap()
            .unwrap()
            .value
            .borrow::<RefreshCause>(),
        Some(RefreshCause::Invalidated(phenix_storage::Notice::Changed(
            _
        )))
    ));
    for v in 10..15 {
        f.put(&f.a, v);
        f.put(&f.b, v);
    }
    for _ in 0..2 {
        assert!(matches!(
            producer
                .poll()
                .unwrap()
                .unwrap()
                .value
                .borrow::<RefreshCause>(),
            Some(RefreshCause::Invalidated(
                phenix_storage::Notice::Overflow { .. }
            ))
        ));
    }
    assert!(producer.poll().unwrap().is_none());
    drop(producer);
    f.put(&f.a, 100);
}

#[test]
fn observation_activation_failure_is_retryable_and_never_bootstraps() {
    let f = Fixture::new(false);
    let (plugin, _) = sum(&f, true);
    let mut runtime = local(plugin);
    assert!(runtime.start().is_err());
    assert!(runtime.start().is_err());
    assert!(runtime.drain(10).is_empty());
}

#[test]
fn producer_admission_is_bounded_and_does_not_execute_processes() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, true);
    let mut runtime = local(plugin);
    assert!(runtime.drain(10).is_empty());
    runtime.start().unwrap();
    assert_eq!(runtime.drain(1).len(), 1);
    f.put(&f.a, 4);
    f.put(&f.b, 7);
    let events = runtime.drain(20);
    assert!(events.len() <= 2);
    runtime
        .kernel()
        .with_session(|s| assert!(s.last_step().is_none()))
        .unwrap();
    settle(&runtime);
    runtime.drain(20);
    settle(&runtime);
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 11);
}

#[test]
fn final_definition_keys_reject_fresh_and_ambiguous_declarations_but_allow_redeployment() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let mut a = local(plugin.clone());
    let mut b = local(plugin);
    a.start().unwrap();
    b.start().unwrap();
    assert_eq!(*a.bind_query(&spec).unwrap().query().unwrap(), 8);
    assert_eq!(*b.bind_query(&spec).unwrap().query().unwrap(), 8);
    let (fresh, _) = sum(&f, false);
    let other = local(fresh);
    assert!(matches!(
        other.bind_query(&spec),
        Err(Error::WrongGeneration)
    ));
    let mut child = NetworkBuilder::new();
    let sum = child
        .derive("sum", (&f.a, &f.b), |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let spec = child.query_spec("query", &sum).unwrap();
    let def = child.build().unwrap();
    let mut parent = NetworkBuilder::new();
    parent.module("one", def.clone()).unwrap();
    parent.module("two", def).unwrap();
    let runtime = local(authored(parent, spec.clone(), None));
    assert!(matches!(
        runtime.bind_query(&spec),
        Err(Error::WrongGeneration)
    ));
}

#[test]
fn missing_redirected_wrong_type_and_fanout_refresh_routes_reject_without_io() {
    for mode in ["missing", "redirected", "wrong_type", "fanout", "valid"] {
        let f = Fixture::new(false);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let route = "sum.sources.refresh".to_owned();
        let mut parent = NetworkBuilder::new();
        let mut child_graph = NetworkBuilder::new();
        let sum = child_graph
            .derive("sum", (&f.a, &f.b), |a: Number, b: Number| a.0 + b.0)
            .unwrap();
        let spec = child_graph.query_spec("query", &sum).unwrap();
        let child = parent
            .module("child", child_graph.build().unwrap())
            .unwrap();
        for name in ["sum.sources.snapshot", "sum.sources.errors", "query"] {
            parent
                .expose_output_dynamic(name, &child.output_endpoint(name).unwrap())
                .unwrap();
        }
        if mode == "valid" || mode == "fanout" {
            parent
                .expose_input(
                    &route,
                    child.input::<RefreshCause>(&route).unwrap(),
                    capacity(),
                )
                .unwrap();
        }
        if mode == "redirected" || mode == "fanout" {
            let unrelated = sink(&mut parent, "unrelated", |_: RefreshCause| {}).unwrap();
            parent
                .expose_input(
                    &route,
                    unrelated.input::<RefreshCause>("run").unwrap(),
                    capacity(),
                )
                .unwrap();
        }
        if mode == "wrong_type" {
            let unrelated = sink(&mut parent, "unrelated", |_: String| {}).unwrap();
            parent
                .expose_input(
                    &route,
                    unrelated.input::<String>("run").unwrap(),
                    capacity(),
                )
                .unwrap();
        }
        let mut runtime = local(authored(parent, spec.clone(), None));
        if mode == "valid" {
            runtime.start().unwrap();
            assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
        } else {
            assert!(
                matches!(
                    runtime.bind_query(&spec),
                    Err(Error::InvalidRefreshRoute(_))
                ),
                "{mode}"
            );
        }
    }
}

struct Offload;
impl ProcessDispatcher for Offload {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        DispatchResult::Completed(std::thread::spawn(move || job.run()).join().unwrap())
    }
}

#[test]
fn local_and_offloaded_demand_and_observer_processes_cannot_query_another_runtime() {
    for offload in [false, true] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (target, spec) = sum(&f, false);
        let mut target = local(target);
        target.start().unwrap();
        let target = Arc::new(target);
        let nested = target.clone();
        let nested_spec = spec.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let checked = calls.clone();
        let mut g = NetworkBuilder::new();
        let group = g.sources("group", (&f.a, &f.b)).unwrap();
        let derived = g
            .derive_from("value", &group, move |a: Number, b: Number| {
                assert!(matches!(
                    nested.bind_query(&nested_spec).unwrap().query(),
                    Err(Error::NestedQuery)
                ));
                checked.fetch_add(1, Ordering::SeqCst);
                a.0 + b.0
            })
            .unwrap();
        let query = g.query_spec("query", &derived).unwrap();
        let observer = group.observation(1);
        let dispatcher: Arc<dyn ProcessDispatcher> = if offload {
            Arc::new(Offload)
        } else {
            Arc::new(LocalProcessDispatcher)
        };
        let mut runtime = build(authored(g, query.clone(), Some(observer)), dispatcher);
        runtime.start().unwrap();
        assert_eq!(*runtime.bind_query(&query).unwrap().query().unwrap(), 8);
        runtime.drain(1);
        settle(&runtime);
        f.put(&f.b, 6);
        runtime.drain(1);
        settle(&runtime);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(*target.bind_query(&spec).unwrap().query().unwrap(), 9);
    }
}

#[test]
fn request_scopes_settle_release_and_reject_stale_or_foreign_capture() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, _) = sum(&f, false);
    let mut runtime = local(plugin);
    runtime.start().unwrap();
    runtime
        .kernel()
        .with_session(|s| {
            let id = s.begin_request(None).unwrap();
            s.inject_request(
                id,
                "sum.sources.refresh",
                Value::local(RefreshCause::Demand),
            )
            .unwrap();
            s.close_request(id).unwrap();
            assert!(matches!(
                s.request_status(id).unwrap(),
                RequestStatus::Pending
            ));
            s.run_until_idle(128);
            let packet = s.take_request_output(id, "query").unwrap().unwrap();
            s.validate_capture(id, "query", &packet).unwrap();
            assert!(
                s.validate_capture(id, "sum.sources.snapshot", &packet)
                    .is_err()
            );
            while s
                .take_request_output(id, "sum.sources.snapshot")
                .unwrap()
                .is_some()
            {}
            assert!(matches!(
                s.request_status(id).unwrap(),
                RequestStatus::Settled { .. }
            ));
            s.release_request(id).unwrap();
            assert!(s.validate_capture(id, "query", &packet).is_err());
            let next = s.begin_request(None).unwrap();
            assert!(s.validate_capture(next, "query", &packet).is_err());
            s.close_request(next).unwrap();
            s.release_request(next).unwrap();
        })
        .unwrap();
    for _ in 0..100 {
        let spec = runtime
            .kernel()
            .export::<QuerySpec<i64>>(&phenix_kernel::Key::new("domain", "derive"))
            .unwrap();
        assert_eq!(*runtime.bind_query(spec).unwrap().query().unwrap(), 8);
    }
}

#[test]
fn query_budget_is_bounded_and_never_returns_partial_or_old_capture() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let mut runtime = local(plugin);
    runtime.start().unwrap();
    assert!(matches!(
        runtime.bind_query(&spec).unwrap().query_with_budget(1),
        Err(Error::Unsettled)
    ));
    runtime
        .kernel()
        .with_session(|s| assert_ne!(s.state(), SessionState::Cancelled))
        .unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
}

struct LifetimeBackend {
    memory: MemoryBackend,
    _lease: Arc<()>,
}
impl Backend for LifetimeBackend {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(&mut self.memory)
    }
    fn observation(&self) -> phenix_storage::Observation {
        self.memory.observation()
    }
}
#[test]
fn backend_lives_until_runtime_and_all_saved_specs_drop_in_either_order() {
    for spec_first in [false, true] {
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let f = Fixture::with_backend(
            true,
            LifetimeBackend {
                memory: MemoryBackend::default(),
                _lease: lease,
            },
        );
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (plugin, spec) = sum(&f, true);
        let mut runtime = local(plugin);
        runtime.start().unwrap();
        let second = spec.clone();
        assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
        drop(f);
        assert!(weak.upgrade().is_some());
        if spec_first {
            drop(spec);
            drop(second);
            assert!(weak.upgrade().is_some());
            drop(runtime);
        } else {
            drop(runtime);
            assert!(weak.upgrade().is_some());
            drop(spec);
            assert!(weak.upgrade().is_some());
            drop(second);
        }
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn query_value_clones_retain_the_actual_application_code_lease() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let mut config = options(&plugin);
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    config.execution.lease = Some(lease);
    let mut runtime = configured(plugin, Arc::new(LocalProcessDispatcher), config);
    runtime.start().unwrap();
    let value = runtime.bind_query(&spec).unwrap().query().unwrap();
    let retained = value.clone();
    drop(runtime);
    drop(spec);
    assert!(weak.upgrade().is_some());
    assert_eq!(*retained, 8);
    drop(value);
    assert!(weak.upgrade().is_some());
    drop(retained);
    assert!(weak.upgrade().is_none());
}

struct CommitAfterSource {
    first: AtomicBool,
    commit: Box<dyn Fn() + Send + Sync>,
}
impl ProcessDispatcher for CommitAfterSource {
    fn dispatch(&self, job: ActivationJob) -> DispatchResult {
        let source = job.metadata().is_some_and(|m| m.path == "sum.sources");
        let completed = job.run();
        if source && self.first.swap(false, Ordering::SeqCst) {
            (self.commit)();
        }
        DispatchResult::Completed(completed)
    }
}

#[test]
fn memory_and_sqlite_queries_return_their_snapshot_not_a_later_commit() {
    for sqlite in [false, true] {
        let root = example::TempRoot::new().unwrap();
        let database = root.path().join("snapshot.sqlite");
        let f = if sqlite {
            let registry = phenix_storage::Registry::new();
            registry.mount_sqlite("main", &database).unwrap();
            Fixture::from_registry(false, registry)
        } else {
            Fixture::new(false)
        };
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let a = f.a.source().address().clone();
        let b = f.b.source().address().clone();
        let writers = if sqlite {
            let registry = phenix_storage::Registry::new();
            registry.mount_sqlite("main", &database).unwrap();
            for address in [&a, &b] {
                registry.register(address, Format.schema()).unwrap();
            }
            registry
        } else {
            f.registry.clone()
        };
        let writer = writers.session([Grant::new(Scope::Mount("main".into()), [Operation::Write])]);
        let (plugin, spec) = sum(&f, false);
        let dispatcher = Arc::new(CommitAfterSource {
            first: AtomicBool::new(true),
            commit: Box::new(move || {
                writer.put(&a, "value", &Number(4), &Format).unwrap();
                writer.put(&b, "value", &Number(7), &Format).unwrap();
            }),
        });
        let mut runtime = build(plugin, dispatcher);
        runtime.start().unwrap();
        let query = runtime.bind_query(&spec).unwrap();
        assert_eq!(*query.query().unwrap(), 8, "sqlite={sqlite}");
        assert_eq!(
            (f.a.get().unwrap().value.0, f.b.get().unwrap().value.0),
            (4, 7)
        );
        assert_eq!(*query.query().unwrap(), 11, "sqlite={sqlite}");
    }
}

#[test]
fn multiple_terminal_activations_never_return_an_old_or_arbitrary_value() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let mut g = NetworkBuilder::new();
    let group = g.sources("values", (&f.a, &f.b)).unwrap();
    let sum = g
        .derive_from("sum", &group, |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let other = g
        .derive_from("other", &group, |a: Number, b: Number| a.0 * b.0)
        .unwrap();
    let spec = g.query_spec("query", &sum).unwrap();
    let tap = g.component("query").unwrap();
    g.connect(other.output(), tap.input("run").unwrap(), capacity())
        .unwrap();
    let mut runtime = local(authored(g, spec.clone(), None));
    runtime.start().unwrap();
    assert!(matches!(
        runtime.bind_query(&spec).unwrap().query(),
        Err(Error::PublicationCount(2))
    ));
    f.put(&f.a, 4);
    assert!(matches!(
        runtime.bind_query(&spec).unwrap().query(),
        Err(Error::PublicationCount(2))
    ));
}

#[test]
fn a_missing_terminal_never_reuses_a_previous_capture() {
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let mut runtime = local(plugin);
    runtime.start().unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
    f.session.delete(f.a.source().address(), "value").unwrap();
    assert!(matches!(
        runtime.bind_query(&spec).unwrap().query(),
        Err(Error::Wave(_))
    ));
    f.put(&f.a, 4);
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 9);
}

#[test]
fn demand_process_cannot_recursively_query_its_own_runtime() {
    type SelfQuery = Option<Box<dyn Fn() -> bool>>;
    thread_local! { static QUERY: std::cell::RefCell<SelfQuery> = const { std::cell::RefCell::new(None) }; }
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            QUERY.with(|q| *q.borrow_mut() = None);
        }
    }
    let _clear = Clear;
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let mut g = NetworkBuilder::new();
    let sum = g
        .derive("sum", (&f.a, &f.b), |a: Number, b: Number| {
            assert!(QUERY.with(|q| q.borrow().as_ref().unwrap()()));
            a.0 + b.0
        })
        .unwrap();
    let spec = g.query_spec("query", &sum).unwrap();
    let mut runtime = local(authored(g, spec.clone(), None));
    runtime.start().unwrap();
    let runtime = std::rc::Rc::new(runtime);
    let owner = runtime.clone();
    let nested = spec.clone();
    QUERY.with(|q| {
        *q.borrow_mut() = Some(Box::new(move || {
            matches!(
                owner.bind_query(&nested).unwrap().query(),
                Err(Error::NestedQuery)
            )
        }))
    });
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
}

#[test]
fn execution_guard_restores_after_process_panic_on_local_and_worker_threads() {
    for offload in [false, true] {
        let f = Fixture::new(false);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let mut g = NetworkBuilder::new();
        let value = g
            .derive("panic", (&f.a,), |_: Number| -> i64 {
                panic!("process fixture")
            })
            .unwrap();
        let spec = g.query_spec("query", &value).unwrap();
        let dispatcher: Arc<dyn ProcessDispatcher> = if offload {
            Arc::new(Offload)
        } else {
            Arc::new(LocalProcessDispatcher)
        };
        let mut failed = build(authored(g, spec.clone(), None), dispatcher);
        failed.start().unwrap();
        assert!(matches!(
            failed.bind_query(&spec).unwrap().query(),
            Err(Error::Wave(_))
        ));
        // Request-local failure must never leave the worker guard set.
        let (plugin, spec) = sum(&f, false);
        let mut healthy = local(plugin);
        healthy.start().unwrap();
        assert_eq!(*healthy.bind_query(&spec).unwrap().query().unwrap(), 8);
    }
}

#[test]
fn late_public_export_collision_rolls_back_source_and_tap_authoring() {
    let f = Fixture::new(false);
    let mut g = NetworkBuilder::new();
    let existing = g.sources("existing", (&f.a,)).unwrap();
    g.expose_output("group.snapshot", existing.output())
        .unwrap();
    assert!(g.sources("group", (&f.a,)).is_err());
    assert!(g.component("group").is_err());
    let sum = g.derive_from("sum", &existing, |a: Number| a.0).unwrap();
    g.expose_output("tap", sum.output()).unwrap();
    assert!(g.query_spec("tap", &sum).is_err());
    assert!(g.component("tap").is_err());
    assert!(g.derive("sum", (&f.a,), |a: Number| a.0).is_err());
    assert!(g.component("sum.sources").is_err());
    g.inspect(&g.component("existing").unwrap()).unwrap();
}

#[test]
fn unwinding_nested_derived_authoring_preserves_existing_and_revokes_aborted_handles() {
    let f = Fixture::new(false);
    let mut graph = NetworkBuilder::new();
    let existing = graph.sources("existing", (&f.a,)).unwrap();
    let mut escaped = None;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), Error> = graph.additions(|graph| {
            let value = graph.derive_from("aborted", &existing, |a: Number| a.0)?;
            escaped = Some(value.clone());
            graph.query_spec("aborted.query", &value)?;
            panic!("abort after nested derived additions");
        });
    }));
    assert!(result.is_err());
    assert!(graph.component("aborted").is_err());
    assert!(graph.component("aborted.query").is_err());
    let escaped = escaped.unwrap();
    assert!(graph.derive("bad", (&escaped,), |a| a).is_err());
    let value = graph
        .derive_from("aborted", &existing, |a: Number| a.0)
        .unwrap();
    graph.query_spec("aborted.query", &value).unwrap();
    assert!(graph.derive("still.bad", (&escaped,), |a| a).is_err());
    graph.build().unwrap();
}

#[test]
fn repeated_root_activations_are_rejected_even_when_the_values_agree() {
    use phenix_graph::{
        Interface, PortDef,
        process::{Process, ProcessContext, ProcessEmission, Step},
    };
    struct Again {
        fired: bool,
        emitted: bool,
    }
    impl Process for Again {
        fn step(&mut self, ctx: &mut ProcessContext) -> Result<Step, String> {
            if self.emitted {
                self.emitted = false;
                return Ok(Step::Complete);
            }
            if ctx.take("run").is_none() {
                return Ok(Step::Finished);
            }
            if self.fired {
                return Ok(Step::Complete);
            }
            self.fired = true;
            self.emitted = true;
            Ok(Step::Emit(ProcessEmission::new(
                "value",
                Value::local(RefreshCause::Demand),
            )))
        }
    }
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    let mut g = NetworkBuilder::new();
    let group = g.sources("values", (&f.a,)).unwrap();
    let sum = g.derive_from("sum", &group, |a: Number| a.0).unwrap();
    let spec = g.query_spec("query", &sum).unwrap();
    let again = g
        .node(
            "again",
            Arc::new(
                Interface::new(
                    [PortDef::local::<(Number,)>("run")],
                    [PortDef::local::<RefreshCause>("value")],
                )
                .unwrap(),
            ),
            Arc::new(|| Again {
                fired: false,
                emitted: false,
            }),
        )
        .unwrap();
    g.connect(group.output(), again.input("run").unwrap(), capacity())
        .unwrap();
    g.connect(
        again.output::<RefreshCause>("value").unwrap(),
        g.component("values").unwrap().input("run").unwrap(),
        capacity(),
    )
    .unwrap();
    let mut runtime = local(authored(g, spec.clone(), None));
    runtime.start().unwrap();
    assert!(matches!(
        runtime.bind_query(&spec).unwrap().query(),
        Err(Error::InvalidPublication)
    ));
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 3);
}

#[test]
fn observer_bootstrap_and_commits_wake_the_native_kernel_driver() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, _) = sum(&f, true);
    let mut runtime = local(plugin);
    runtime.start().unwrap();
    let wake = phenix_kernel::WakeHandle::default();
    let stop = wake.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let driver = std::thread::spawn(move || {
        runtime.run_until(&wake, |event| tx.send(event.result.is_ok()).unwrap())
    });
    assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap());
    f.put(&f.b, 7);
    let published = rx.recv_timeout(std::time::Duration::from_secs(5));
    stop.shutdown();
    driver.join().unwrap().unwrap();
    assert!(published.unwrap());
}

#[test]
fn unknown_producer_route_rejects_before_activation() {
    use phenix_graph::compose::NetworkPatch;
    use phenix_kernel::{CandidateArtifacts, Contribution, Kernel};
    use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
    struct Invalid(Arc<AtomicUsize>);
    impl PluginInstance for Invalid {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let starts = self.0.clone();
            registrar.add(Contribution {
                id: "invalid".into(),
                requires: vec![],
                routes: vec!["missing".into()],
                patch: NetworkPatch::new([]),
                prepare: Some(Box::new(move |_| {
                    let starts = starts.clone();
                    let mut artifacts = CandidateArtifacts::default();
                    artifacts.activate(move || {
                        starts.fetch_add(1, Ordering::SeqCst);
                        Err("must not activate".into())
                    })?;
                    Ok(artifacts)
                })),
            })
        }
    }
    let starts = Arc::new(AtomicUsize::new(0));
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("invalid", |p: Invalid| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, Invalid(starts.clone())).unwrap();
    assert!(
        Kernel::build(
            "invalid",
            plugins.snapshots(),
            Arc::new(LocalProcessDispatcher)
        )
        .is_err()
    );
    assert_eq!(starts.load(Ordering::SeqCst), 0);
}

#[test]
fn process_panic_fails_only_its_demand_and_subsequent_demand_recovers() {
    use phenix_graph::{
        Interface, PortDef,
        process::{Process, ProcessContext, Step},
    };
    struct Counter {
        count: usize,
        log: Arc<Mutex<Vec<usize>>>,
    }
    impl Process for Counter {
        fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
            if context.take("run").is_none() {
                return Ok(Step::Finished);
            }
            self.count += 1;
            self.log.lock().unwrap().push(self.count);
            Ok(Step::Complete)
        }
    }
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    let first = Arc::new(AtomicBool::new(true));
    let effects = Arc::new(AtomicUsize::new(0));
    let committed = effects.clone();
    let mut graph = NetworkBuilder::new();
    let log = Arc::new(Mutex::new(Vec::new()));
    let counter_log = log.clone();
    let counter = graph
        .node(
            "counter",
            Arc::new(Interface::new([PortDef::local::<()>("run")], []).unwrap()),
            Arc::new(move || Counter {
                count: 0,
                log: counter_log.clone(),
            }),
        )
        .unwrap();
    graph
        .expose_input("counter", counter.input::<()>("run").unwrap(), capacity())
        .unwrap();
    let value = graph
        .derive("value", (&f.a,), move |a: Number| {
            committed.fetch_add(1, Ordering::SeqCst);
            assert!(!first.swap(false, Ordering::SeqCst), "first demand panics");
            a.0
        })
        .unwrap();
    let spec = graph.query_spec("query", &value).unwrap();
    let mut runtime = local(authored(graph, spec.clone(), None));
    runtime.start().unwrap();
    runtime
        .kernel()
        .inject("counter", Value::local(()))
        .unwrap();
    runtime.kernel().run_steps(16).unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert!(matches!(query.query(), Err(Error::Wave(_))));
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    runtime
        .kernel()
        .inject("counter", Value::local(()))
        .unwrap();
    runtime.kernel().run_steps(16).unwrap();
    assert_eq!(*query.query().unwrap(), 3);
    assert_eq!(effects.load(Ordering::SeqCst), 2);
    assert_eq!(*log.lock().unwrap(), [1, 2]);
}

#[test]
fn cancelled_query_transfers_handed_job_inventory_and_rejects_late_completion() {
    struct DeferredOnce(Mutex<Option<ActivationJob>>, AtomicBool);
    impl ProcessDispatcher for DeferredOnce {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            if self.1.swap(false, Ordering::SeqCst) {
                *self.0.lock().unwrap() = Some(job);
                DispatchResult::Deferred
            } else {
                DispatchResult::Completed(job.run())
            }
        }
    }
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, spec) = sum(&f, false);
    let dispatcher = Arc::new(DeferredOnce(Mutex::new(None), AtomicBool::new(true)));
    let mut runtime = build(plugin, dispatcher.clone());
    runtime.start().unwrap();
    let error = runtime
        .bind_query(&spec)
        .unwrap()
        .query_with_budget(1)
        .unwrap_err();
    let job = dispatcher.0.lock().unwrap().take().unwrap();
    let request = job.metadata().unwrap().request.unwrap();
    match error {
        Error::RequestInterrupted {
            cause,
            cancelled_jobs,
            disposal,
        } => {
            assert!(matches!(*cause, Error::Unsettled));
            assert_eq!(cancelled_jobs, [job.ticket()]);
            assert_eq!(disposal.handed_jobs, 1);
        }
        other => panic!("lost cancellation inventory: {other:?}"),
    }
    runtime
        .kernel()
        .with_session(|session| {
            assert!(matches!(
                session.request_status(request),
                Err(phenix_graph::runtime::RequestError::Stale)
            ));
            assert!(session.complete_job(job.run()).is_err());
            assert_ne!(session.state(), SessionState::Cancelled);
        })
        .unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
}

#[test]
fn automatic_failure_snapshots_cancelled_peer_jobs_before_releasing_the_request() {
    struct DeferredPeer {
        job: Mutex<Option<ActivationJob>>,
        first: AtomicBool,
    }
    impl ProcessDispatcher for DeferredPeer {
        fn dispatch(&self, job: ActivationJob) -> DispatchResult {
            if job.metadata().is_some_and(|m| m.path == "a.held")
                && self.first.swap(false, Ordering::SeqCst)
            {
                *self.job.lock().unwrap() = Some(job);
                DispatchResult::Deferred
            } else {
                DispatchResult::Completed(job.run())
            }
        }
    }
    let f = Fixture::new(false);
    f.put(&f.a, 3);
    let mut graph = NetworkBuilder::new();
    let group = graph.sources("values", (&f.a,)).unwrap();
    let held = graph
        .derive_from("a.held", &group, |a: Number| a.0)
        .unwrap();
    graph.query_spec("held.query", &held).unwrap();
    let first = AtomicBool::new(true);
    let failed = graph
        .derive_from("z.fail", &group, move |a: Number| {
            assert!(
                !first.swap(false, Ordering::SeqCst),
                "fail with a peer in flight"
            );
            a.0
        })
        .unwrap();
    let spec = graph.query_spec("query", &failed).unwrap();
    let dispatcher = Arc::new(DeferredPeer {
        job: Mutex::new(None),
        first: AtomicBool::new(true),
    });
    let mut runtime = build(authored(graph, spec.clone(), None), dispatcher.clone());
    runtime.start().unwrap();
    let error = runtime.bind_query(&spec).unwrap().query().unwrap_err();
    let job = dispatcher.job.lock().unwrap().take().unwrap();
    match error {
        Error::RequestInterrupted {
            cause,
            cancelled_jobs,
            disposal,
        } => {
            assert!(matches!(*cause, Error::Wave(e) if e.join(" ").contains("panicked")));
            assert_eq!(cancelled_jobs, [job.ticket()]);
            assert_eq!(disposal.handed_jobs, 1);
        }
        other => panic!("lost failure inventory: {other:?}"),
    }
    runtime
        .kernel()
        .with_session(|session| {
            assert!(matches!(
                session.request_status(job.metadata().unwrap().request.unwrap()),
                Err(phenix_graph::runtime::RequestError::Stale)
            ));
            assert!(session.complete_job(job.run()).is_err());
        })
        .unwrap();
    assert_eq!(*runtime.bind_query(&spec).unwrap().query().unwrap(), 3);
}
