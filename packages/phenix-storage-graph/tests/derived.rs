use phenix_graph::{
    Deployment, Emission, Executor, GraphBuilder, InvocationCtx, Layout, LocalExecutor, NodeAction,
};
use phenix_kernel::{Activate, Contribution, WakeHandle};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{
    Address, Backend, Codec, Grant, MemoryBackend, Operation, Registry, Revisioned, Schema, Scope,
    Session, TypedKv,
    scoped::{CellSpec, Identity, ReadCell, ResourceCatalog, Stored},
};
use phenix_storage_graph::derived::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

#[derive(Clone, Debug)]
struct Number(i64);
impl std::ops::Add for Number {
    type Output = i64;
    fn add(self, rhs: Self) -> i64 {
        self.0 + rhs.0
    }
}
#[derive(Default)]
struct Format;
impl Stored for Number {
    type Format = Format;
}
impl Codec for Format {
    type Value = Number;
    fn schema(&self) -> Schema {
        Schema {
            name: "number".into(),
            revision: 1,
            structure: "i64".into(),
            codec: "be".into(),
        }
    }
    fn encode(&self, value: &Number) -> phenix_storage::Result<Vec<u8>> {
        Ok(value.0.to_be_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<Number> {
        Ok(Number(i64::from_be_bytes(bytes.try_into().map_err(
            |_| phenix_storage::Error::Codec("length".into()),
        )?)))
    }
}
const A: CellSpec<Number> = CellSpec::project_state("test/a");
const B: CellSpec<Number> = CellSpec::project_state("test/b");
struct Fixture {
    registry: Registry,
    session: Session,
    a: ReadCell<Number>,
    b: ReadCell<Number>,
}
impl Fixture {
    fn new(subscribe: bool) -> Self {
        Self::with_backend(subscribe, MemoryBackend::default())
    }
    fn with_backend(subscribe: bool, backend: impl Backend + 'static) -> Self {
        let registry = Registry::new();
        registry.mount("main", backend).unwrap();
        Self::from_registry(subscribe, registry)
    }
    fn from_registry(subscribe: bool, registry: Registry) -> Self {
        let identity = Identity::user("u").unwrap().project("p").unwrap();
        let mut catalog = ResourceCatalog::new();
        for (spec, name) in [(A, "a"), (B, "b")] {
            let address = Address::parse(&format!("store://main/{name}")).unwrap();
            registry.register(&address, Format.schema()).unwrap();
            catalog
                .bind(spec.resource_spec().resolve(&identity).unwrap(), address)
                .unwrap();
        }
        let session = registry.session([Grant::new(
            Scope::Mount("main".into()),
            [
                Operation::Read,
                Operation::Write,
                Operation::Delete,
                Operation::Subscribe,
            ],
        )]);
        let operations = if subscribe {
            vec![Operation::Read, Operation::Subscribe]
        } else {
            vec![Operation::Read]
        };
        let storage = catalog.freeze().access(
            registry.session([Grant::new(Scope::Mount("main".into()), operations)]),
            identity,
        );
        let (a, b) = storage.bind((A.read(), B.read())).unwrap();
        Self {
            registry,
            session,
            a,
            b,
        }
    }
    fn put(&self, cell: &ReadCell<Number>, value: i64) {
        self.session
            .put(cell.source().address(), "value", &Number(value), &Format)
            .unwrap();
    }
}

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
    fn get(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<Option<Vec<u8>>> {
        self.memory.get(dataset, key)
    }
    fn put(&mut self, dataset: &str, key: &str, value: Vec<u8>) -> phenix_storage::Result<()> {
        self.memory.put(dataset, key, value)
    }
    fn delete(&mut self, dataset: &str, key: &str) -> phenix_storage::Result<bool> {
        self.memory.delete(dataset, key)
    }
}

#[test]
fn one_batch_per_query_no_build_binding_reads_and_runtime_native_denial_recovers() {
    let reads = Arc::new(AtomicUsize::new(0));
    let denied = Arc::new(AtomicBool::new(false));
    let f = Fixture::with_backend(
        false,
        ReadProbe {
            memory: MemoryBackend::default(),
            reads: reads.clone(),
            denied: denied.clone(),
        },
    );
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, saved) = sum_plugin(&f);
    let mut runtime = build(plugin.clone(), Arc::new(LocalExecutor));
    let spec = saved.lock().unwrap().clone().unwrap();
    runtime.bind_query(&spec).unwrap();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    runtime.start().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert_eq!(query.query().unwrap(), 8);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    denied.store(true, Ordering::SeqCst);
    assert!(
        matches!(query.query(), Err(Error::Wave(e)) if e.join(" ").contains("source[Storage]: Denied"))
    );
    denied.store(false, Ordering::SeqCst);
    f.put(&f.a, 4);
    assert_eq!(query.query().unwrap(), 9);
    let other = build(plugin, Arc::new(LocalExecutor));
    assert!(matches!(
        other.bind_query(&spec),
        Err(Error::WrongGeneration)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 3);
}

#[test]
fn mixed_snapshot_sources_reject_before_topology_mutation() {
    let first = Fixture::new(true);
    let second = Fixture::new(true);
    let mut graph = GraphBuilder::new("mixed");
    assert!(matches!(
        graph.sources("group", (&first.a, &second.b)),
        Err(Error::Storage(phenix_storage::Error::MixedSnapshotSources))
    ));
    graph.sources("group", (&first.a, &first.b)).unwrap();
    graph.compile().unwrap();
}

#[test]
fn invalid_producer_route_preflight_never_activates() {
    let activated = Arc::new(AtomicUsize::new(0));
    let calls = activated.clone();
    let plugin = Plugin {
        compose: Arc::new(|_| Ok(())),
        routes: vec!["missing".into()],
        activate: Some(Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("should not activate".into())
        })),
    };
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("invalid", |p: Plugin| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, plugin).unwrap();
    assert!(
        DerivedRuntime::build("invalid", plugins.snapshots(), Arc::new(LocalExecutor)).is_err()
    );
    assert_eq!(activated.load(Ordering::SeqCst), 0);
}
type ComposeGraph = dyn Fn(&mut GraphBuilder) -> Result<(), String> + Send + Sync;

#[derive(Clone)]
struct Plugin {
    compose: Arc<ComposeGraph>,
    activate: Option<Arc<Activate>>,
    routes: Vec<String>,
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let compose = self.compose.clone();
        let activate = self.activate.clone();
        registrar.add(Contribution {
            id: "derive".into(),
            requires: vec![],
            routes: self.routes.clone(),
            compose: Box::new(move |context| {
                compose(context.graph)?;
                if let Some(activate) = activate.clone() {
                    context.activate(move || activate())?;
                }
                Ok(())
            }),
        })
    }
}
fn build(plugin: Plugin, executor: Arc<dyn Executor>) -> DerivedRuntime {
    let mut catalog = PluginCatalog::new();
    let kind = catalog
        .register("domain", |plugin: Plugin| Ok(plugin))
        .unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, plugin).unwrap();
    DerivedRuntime::build("derived", plugins.snapshots(), executor).unwrap()
}
fn demand(
    compose: impl Fn(&mut GraphBuilder) -> Result<(), String> + Send + Sync + 'static,
) -> Plugin {
    Plugin {
        compose: Arc::new(compose),
        activate: None,
        routes: vec![],
    }
}
fn sum_plugin(f: &Fixture) -> (Plugin, Arc<Mutex<Option<QuerySpec<i64>>>>) {
    let (a, b) = (f.a.clone(), f.b.clone());
    let spec = Arc::new(Mutex::new(None));
    let saved = spec.clone();
    (
        demand(move |g| {
            let sum = g
                .derive("sum", (&a, &b), |a, b| a + b)
                .map_err(|e| e.to_string())?;
            *saved.lock().unwrap() = Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
            Ok(())
        }),
        spec,
    )
}

#[test]
fn demand_arithmetic_changes_start_guard_generation_and_recovery() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, saved) = sum_plugin(&f);
    let mut runtime = build(plugin.clone(), Arc::new(LocalExecutor));
    let spec = saved.lock().unwrap().clone().unwrap();
    assert!(
        matches!(runtime.bind_query(&spec).unwrap().query(), Err(Error::Graph(e)) if e.contains("not started"))
    );
    runtime.start().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert_eq!(query.query().unwrap(), 8);
    f.put(&f.a, 4);
    assert_eq!(query.query().unwrap(), 9);
    f.put(&f.b, 7);
    assert_eq!(query.query().unwrap(), 11);
    f.session.delete(f.a.source().address(), "value").unwrap();
    assert!(
        matches!(query.query(), Err(Error::Wave(e)) if e.join(" ").contains("source[MissingInput]"))
    );
    f.put(&f.a, 10);
    assert_eq!(query.query().unwrap(), 17);
    // Same names and same contribution closure, but freshly composed generation.
    let other = build(plugin, Arc::new(LocalExecutor));
    assert!(matches!(
        other.bind_query(&spec),
        Err(Error::WrongGeneration)
    ));
    assert!(matches!(
        runtime.bind_query(saved.lock().unwrap().as_ref().unwrap()),
        Err(Error::WrongGeneration)
    ));
}

struct CommitAfterSource {
    first: AtomicBool,
    commit: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
}
impl Executor for CommitAfterSource {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        let source = ctx.shared::<RefreshCause>(0).is_ok();
        let emissions = action.execute(ctx)?;
        if source && self.first.swap(false, Ordering::SeqCst) {
            // The source has finished its read and released all storage locks,
            // but the executor has not returned its tuple to the wave yet.
            (self.commit)()?;
        }
        Ok(emissions)
    }
}
struct RemoveDatabase(std::path::PathBuf);
impl Drop for RemoveDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn query_returns_its_snapshot_not_a_commit_after_the_source_read() {
    for sqlite in [false, true] {
        let path = RemoveDatabase(std::env::temp_dir().join(format!(
            "phenix-derived-query-snapshot-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
        )));
        let f = if sqlite {
            let registry = Registry::new();
            registry.mount_sqlite("main", &path.0).unwrap();
            Fixture::from_registry(false, registry)
        } else {
            Fixture::new(false)
        };
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (a_address, b_address) = (
            f.a.source().address().clone(),
            f.b.source().address().clone(),
        );
        let writers = if sqlite {
            // A separate registry owns an independent SQLite connection to the
            // same file, not just a second session sharing the reader's lock.
            let registry = Registry::new();
            registry.mount_sqlite("main", &path.0).unwrap();
            for address in [&a_address, &b_address] {
                registry.register(address, Format.schema()).unwrap();
            }
            registry
        } else {
            f.registry.clone()
        };
        let writer = writers.session([Grant::new(Scope::Mount("main".into()), [Operation::Write])]);
        let commits = Arc::new(AtomicUsize::new(0));
        let written = commits.clone();
        let executor = Arc::new(CommitAfterSource {
            first: AtomicBool::new(true),
            commit: Box::new(move || {
                writer
                    .put(&a_address, "value", &Number(4), &Format)
                    .map_err(|e| e.to_string())?;
                writer
                    .put(&b_address, "value", &Number(7), &Format)
                    .map_err(|e| e.to_string())?;
                written.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        });
        let (a, b) = (f.a.clone(), f.b.clone());
        let saved = Arc::new(Mutex::new(None));
        let specs = saved.clone();
        let computed_after_commit = commits.clone();
        let mut runtime = build(
            demand(move |g| {
                let committed = computed_after_commit.clone();
                let pair = g
                    .derive("pair", (&a, &b), move |a: Number, b: Number| {
                        assert_eq!(committed.load(Ordering::SeqCst), 1);
                        (a.0, b.0)
                    })
                    .map_err(|e| e.to_string())?;
                *specs.lock().unwrap() =
                    Some(g.query_spec("query", &pair).map_err(|e| e.to_string())?);
                Ok(())
            }),
            executor,
        );
        runtime.start().unwrap();
        let spec = saved.lock().unwrap().take().unwrap();
        let query = runtime.bind_query(&spec).unwrap();
        assert_eq!(commits.load(Ordering::SeqCst), 0);
        assert_eq!(query.query().unwrap(), (3, 5), "sqlite={sqlite}");
        assert_eq!(commits.load(Ordering::SeqCst), 1);
        // Both writes have already committed when the first query returns. Its
        // result is the wave's coherent snapshot, not wall-clock freshness.
        assert_eq!(
            (f.a.get().unwrap().value.0, f.b.get().unwrap().value.0),
            (4, 7)
        );
        assert_eq!(query.query().unwrap(), (4, 7), "sqlite={sqlite}");
        assert_eq!(commits.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn same_mount_sources_keep_separate_dataset_session_authority() {
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
    assert!(matches!(
        only_a.bind(B.read()),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
    assert!(matches!(
        only_b.bind(A.read()),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
    let saved = Arc::new(Mutex::new(None));
    let specs = saved.clone();
    let observation = Arc::new(Mutex::new(None));
    let request = observation.clone();
    let mut runtime = build(
        demand(move |g| {
            let group = g.sources("values", (&a, &b)).map_err(|e| e.to_string())?;
            let sum = g
                .derive_from("sum", &group, |a, b| a + b)
                .map_err(|e| e.to_string())?;
            *specs.lock().unwrap() = Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
            *request.lock().unwrap() = Some(sum.observation(1));
            Ok(())
        }),
        Arc::new(LocalExecutor),
    );
    runtime.start().unwrap();
    let spec = saved.lock().unwrap().take().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert_eq!(query.query().unwrap(), 8);
    f.put(&f.b, 7);
    assert_eq!(query.query().unwrap(), 10);
    // A has Subscribe, but B does not. Grouping must not union either session's
    // grants, even after a successful cross-session atomic source read.
    assert!(matches!(
        observation.lock().unwrap().as_ref().unwrap().activate(),
        Err(Error::Storage(phenix_storage::Error::Denied))
    ));
    assert!(matches!(
        only_a.bind(B.read()),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
    assert!(matches!(
        only_b.bind(A.read()),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
    assert_eq!(query.query().unwrap(), 10);
}

#[test]
fn shared_siblings_chain_diamond_arity_and_terminal_tap_with_fanout() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (a, b) = (f.a.clone(), f.b.clone());
    let saved = Arc::new(Mutex::new(Vec::new()));
    let specs = saved.clone();
    let consumed = Arc::new(Mutex::new(Vec::new()));
    let log = consumed.clone();
    let mut runtime = build(
        demand(move |g| {
            let result = (|| -> Result<(), Error> {
                let group = g.sources("values", (&a, &b))?;
                let sum = g.derive_from("sum", &group, |a: Number, b: Number| a.0 + b.0)?;
                let diff = g.derive_from("diff", &group, |a: Number, b: Number| a.0 - b.0)?;
                let twice = g.derive("twice", (&sum,), |v| v * 2)?;
                let both = g.derive("both", (&sum, &diff), |s, d| s + d)?;
                let diamond = g.derive("diamond", (&sum, &twice), |s, d| s + d)?;
                let three = g.derive("three", (&sum, &diff, &twice), |s, d, t| s + d + t)?;
                let four = g.derive("four", (&sum, &diff, &twice, &both), |s, d, t, b| {
                    s + d + t + b
                })?;
                let stored3 = g.derive(
                    "stored3",
                    (&a, &b, &a),
                    |a: Number, b: Number, c: Number| a.0 + b.0 + c.0,
                )?;
                let stored4 = g.derive(
                    "stored4",
                    (&a, &b, &a, &b),
                    |a: Number, b: Number, c: Number, d: Number| a.0 + b.0 + c.0 + d.0,
                )?;
                let log = log.clone();
                let sink = g
                    .node("consumer", move |value: i64| {
                        log.lock().unwrap().push(value)
                    })
                    .map_err(|e| Error::Graph(e.to_string()))?;
                g.connect(sum.output(), sink.inlet())?;
                let mut specs = specs.lock().unwrap();
                for (name, value) in [
                    ("qsum", &sum),
                    ("qboth", &both),
                    ("qdiamond", &diamond),
                    ("qthree", &three),
                    ("qfour", &four),
                    ("qs3", &stored3),
                    ("qs4", &stored4),
                ] {
                    specs.push(g.query_spec(name, value)?);
                }
                Ok(())
            })();
            result.map_err(|e| e.to_string())
        }),
        Arc::new(LocalExecutor),
    );
    runtime.start().unwrap();
    for (spec, expected) in saved.lock().unwrap().iter().zip([8, 6, 24, 22, 28, 11, 16]) {
        assert_eq!(runtime.bind_query(spec).unwrap().query().unwrap(), expected);
    }
    assert_eq!(*consumed.lock().unwrap(), vec![8; 5]);
}

#[test]
fn optional_and_codec_failures_are_native_wave_failures_not_result_values() {
    let f = Fixture::new(true);
    f.put(&f.b, 5);
    let (a, b) = (f.a.optional(), f.b.clone());
    let saved = Arc::new(Mutex::new(None));
    let specs = saved.clone();
    let mut runtime = build(
        demand(move |g| {
            let sum = g
                .derive("optional", (&a, &b), |a: Option<Number>, b: Number| {
                    a.map_or(0, |a| a.0) + b.0
                })
                .map_err(|e| e.to_string())?;
            *specs.lock().unwrap() = Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
            Ok(())
        }),
        Arc::new(LocalExecutor),
    );
    runtime.start().unwrap();
    let spec = saved.lock().unwrap().clone().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert_eq!(query.query().unwrap(), 5);
    struct Bad;
    impl Codec for Bad {
        type Value = Number;
        fn schema(&self) -> Schema {
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
        matches!(query.query(), Err(Error::Wave(e)) if e.join(" ").contains("source[Storage]: Codec"))
    );
    f.put(&f.b, 9);
    assert_eq!(query.query().unwrap(), 9);
    f.session
        .put(f.a.source().address(), "value", &Number(0), &Bad)
        .unwrap();
    assert!(
        matches!(query.query(), Err(Error::Wave(e)) if e.join(" ").contains("source[Storage]: Codec"))
    );
    f.put(&f.a, 2);
    assert_eq!(query.query().unwrap(), 11);
}

#[test]
fn rejected_roots_foreign_handles_and_collisions_do_not_mutate_or_revoke_user_handles() {
    let f = Fixture::new(true);
    let mut graph = GraphBuilder::new("authoring");
    let user = graph.node("user", |v: (i64, i64)| v.0 + v.1).unwrap();
    let sum = graph
        .derive("sum", (&f.a, &f.b), |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let other = graph.derive("other", (&f.a,), |a: Number| a.0).unwrap();
    assert!(matches!(
        graph.derive("bad", (&sum, &other), |s, o| s + o),
        Err(Error::MultipleRefreshRoots)
    ));
    graph.node("bad", |v: i64| v).unwrap();
    // Failed source expansion must not leave its .sources node or trigger behind.
    assert!(graph.derive("user", (&f.a,), |a: Number| a.0).is_err());
    graph.sources("user.sources", (&f.a,)).unwrap();
    let configured = user.input(Layout::fields()).unwrap();
    assert!(configured.name() == "user");
    let mut foreign = GraphBuilder::new("foreign");
    assert!(foreign.derive("foreign", (&sum,), |s| s).is_err());
    foreign.node("foreign", |s: i64| s).unwrap();
    graph.compile().unwrap();
}

#[test]
fn denied_binding_and_observation_fail_at_native_authority_boundary() {
    let f = Fixture::new(false);
    let mut graph = GraphBuilder::new("denied");
    let group = graph.sources("group", (&f.a, &f.b)).unwrap();
    assert!(matches!(
        group.observation(1).activate(),
        Err(Error::Storage(phenix_storage::Error::Denied))
    ));
    let identity = Identity::user("u").unwrap().project("p").unwrap();
    let mut catalog = ResourceCatalog::new();
    catalog
        .bind(
            A.resource_spec().resolve(&identity).unwrap(),
            f.a.source().address().clone(),
        )
        .unwrap();
    let denied = catalog.freeze().access(f.registry.session([]), identity);
    assert!(matches!(
        denied.bind(A.read()),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
}

#[test]
fn partial_subscription_activation_failure_can_retry_without_bootstrap() {
    let f = Fixture::new(true);
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
    let session = f.registry.session([
        Grant::new(
            Scope::Dataset(f.a.source().address().clone()),
            [Operation::Read, Operation::Subscribe],
        ),
        Grant::new(
            Scope::Dataset(f.b.source().address().clone()),
            [Operation::Read],
        ),
    ]);
    let storage = catalog.freeze().access(session, identity);
    let (a, b) = storage.bind((A.read(), B.read())).unwrap();
    let mut graph = GraphBuilder::new("partial");
    let group = graph.sources("group", (&a, &b)).unwrap();
    for _ in 0..2 {
        // A subscribes successfully, B fails; collection drops the acquired A
        // binding and never returns a producer or queues an observable bootstrap.
        assert!(matches!(
            group.observation(1).activate(),
            Err(Error::Storage(phenix_storage::Error::Denied))
        ));
    }
    let single = graph.sources("single", (&a,)).unwrap();
    let mut producer = single.observation(1).activate().unwrap();
    assert!(producer.poll().unwrap().is_some());
    assert!(producer.poll().unwrap().is_none());
    drop(producer);
    f.put(&f.a, 10);
}

struct TerminalFixture {
    dispatches: AtomicUsize,
    copies: usize,
}

#[derive(Default)]
struct ReplayTerminal {
    dispatches: AtomicUsize,
    saved: Mutex<Option<Vec<Emission>>>,
}
impl Executor for ReplayTerminal {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        let values = action.execute(ctx)?;
        if self.dispatches.fetch_add(1, Ordering::SeqCst) % 3 == 2 {
            let mut saved = self.saved.lock().unwrap();
            if let Some(previous) = &*saved {
                return Ok(previous.clone());
            }
            *saved = Some(values.clone());
        }
        Ok(values)
    }
}
#[test]
fn earlier_terminal_publications_are_not_accepted_as_fresh() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (plugin, saved) = sum_plugin(&f);
    let mut runtime = build(plugin, Arc::new(ReplayTerminal::default()));
    runtime.start().unwrap();
    let spec = saved.lock().unwrap().clone().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert_eq!(query.query().unwrap(), 8);
    f.put(&f.a, 4);
    assert!(matches!(query.query(), Err(Error::InvalidPublication)));
}
impl Executor for TerminalFixture {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        let values = action.execute(ctx)?;
        if self.dispatches.fetch_add(1, Ordering::SeqCst) == 2 {
            Ok((0..self.copies).flat_map(|_| values.clone()).collect())
        } else {
            Ok(values)
        }
    }
}
#[test]
fn zero_and_multiple_publications_never_return_an_old_result() {
    for copies in [0, 2] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (plugin, saved) = sum_plugin(&f);
        let mut runtime = build(
            plugin,
            Arc::new(TerminalFixture {
                dispatches: AtomicUsize::new(0),
                copies,
            }),
        );
        runtime.start().unwrap();
        let spec = saved.lock().unwrap().clone().unwrap();
        let query = runtime.bind_query(&spec).unwrap();
        let result = query.query();
        if copies == 0 {
            assert!(
                matches!(result, Err(Error::PublicationCount(0))),
                "{result:?}"
            );
        } else {
            // The ordinary engine rejects duplicate slots before a successful report.
            assert!(
                matches!(result, Err(Error::Wave(ref e)) if e.join(" ").contains("duplicate output")),
                "{result:?}"
            );
        }
        assert_eq!(query.query().unwrap(), 8);
    }
}

#[test]
fn terminal_multiple_cohorts_and_single_wrong_cohort_fail_validation() {
    for keep_direct in [false, true] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (a, b) = (f.a.clone(), f.b.clone());
        let saved = Arc::new(Mutex::new(None));
        let specs = saved.clone();
        let mut runtime = build(
            demand(move |g| {
                let sum = g
                    .derive("sum", (&a, &b), |a, b| a + b)
                    .map_err(|e| e.to_string())?;
                *specs.lock().unwrap() =
                    Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
                if !keep_direct {
                    g.disconnect(sum.output(), g.local_input::<i64>("query", "run", "value")?)?;
                }
                // Deliberately modify ordinary topology after declaring the terminal tap:
                // it now receives the same value under two distinct, valid cohorts.
                g.add(
                    "fresh",
                    phenix_graph::NodeTemplate::local_function::<i64, i64, i64, _>(|v| v)
                        .with_fresh_cohort(),
                )
                .map_err(|e| e.to_string())?;
                g.connect(sum.output(), g.local_input::<i64>("fresh", "run", "value")?)?;
                g.connect(
                    g.local_output::<i64>("fresh", "value")?,
                    g.local_input::<i64>("query", "run", "value")?,
                )?;
                Ok(())
            }),
            Arc::new(LocalExecutor),
        );
        runtime.start().unwrap();
        let spec = saved.lock().unwrap().clone().unwrap();
        let result = runtime.bind_query(&spec).unwrap().query();
        if keep_direct {
            assert!(
                matches!(result, Err(Error::PublicationCount(2))),
                "{result:?}"
            );
        } else {
            assert!(
                matches!(result, Err(Error::InvalidPublication)),
                "{result:?}"
            );
        }
    }
}

fn observer_plugin(f: &Fixture, log: Arc<Mutex<Vec<i64>>>, capacity: usize) -> Plugin {
    observer_named(f, log, capacity, "sum")
}
fn observer_named(f: &Fixture, log: Arc<Mutex<Vec<i64>>>, capacity: usize, name: &str) -> Plugin {
    let (a, b) = (f.a.clone(), f.b.clone());
    let request = Arc::new(Mutex::new(None::<Observation>));
    let activate_request = request.clone();
    let route = format!("{name}.sources.refresh");
    let name = name.to_owned();
    Plugin {
        routes: vec![route],
        compose: Arc::new(move |g| {
            let sum = g
                .derive(&name, (&a, &b), |a, b| a + b)
                .map_err(|e| e.to_string())?;
            *request.lock().unwrap() = Some(sum.observation(capacity));
            let log = log.clone();
            let sink = g
                .node(format!("{name}.sink"), move |v: i64| {
                    log.lock().unwrap().push(v)
                })
                .map_err(|e| e.to_string())?;
            g.connect(sum.output(), sink.inlet())?;
            Ok(())
        }),
        activate: Some(Arc::new(move || {
            activate_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .activate()
                .map_err(|e| e.to_string())
        })),
    }
}
#[test]
fn observer_subscribe_before_bootstrap_overflow_both_sources_and_teardown() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = build(observer_plugin(&f, log.clone(), 1), Arc::new(LocalExecutor));
    f.put(&f.a, 4); // No activation or computation during build.
    assert!(runtime.drain(10).is_empty());
    assert!(log.lock().unwrap().is_empty());
    runtime.start().unwrap();
    f.put(&f.b, 6); // After subscription but before initial read.
    assert_eq!(runtime.drain(1).len(), 1);
    assert_eq!(*log.lock().unwrap(), vec![10]);
    f.put(&f.a, 10);
    f.put(&f.a, 11);
    f.put(&f.b, 20);
    f.put(&f.b, 21);
    let events = runtime.drain(2);
    assert_eq!(events.len(), 2);
    assert!(events.into_iter().all(|e| e.result.unwrap().success()));
    assert_eq!(*log.lock().unwrap(), vec![10, 32, 32]);
    assert!(runtime.drain(10).is_empty());
    drop(runtime);
    f.put(&f.a, 100);
    assert_eq!(log.lock().unwrap().len(), 3);
}

#[test]
fn observer_bootstrap_and_later_writes_wake_existing_kernel_driver() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let mut runtime = build(
        observer_plugin(&f, Arc::default(), 1),
        Arc::new(LocalExecutor),
    );
    runtime.start().unwrap();
    let wake = WakeHandle::default();
    let stop = wake.clone();
    let (tx, rx) = mpsc::channel();
    let driver = std::thread::spawn(move || {
        runtime.run_until(&wake, |event| {
            tx.send(event.result.unwrap().success()).unwrap();
        })
    });
    assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap());
    f.put(&f.b, 7);
    let result = rx.recv_timeout(Duration::from_secs(5));
    stop.shutdown();
    driver.join().unwrap().unwrap();
    assert!(result.unwrap());
}

#[test]
fn missing_subscribe_start_failure_is_retryable_and_never_bootstraps() {
    let f = Fixture::new(false);
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = build(observer_plugin(&f, log.clone(), 1), Arc::new(LocalExecutor));
    assert!(runtime.start().is_err());
    assert!(runtime.start().is_err());
    assert!(runtime.drain(10).is_empty());
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn independent_observers_are_fair_with_bounded_tails_and_drop_together() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let logs = [
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
    ];
    let mut catalog = PluginCatalog::new();
    let one = catalog.register("one", |p: Plugin| Ok(p)).unwrap();
    let two = catalog.register("two", |p: Plugin| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins
        .load(&one, observer_named(&f, logs[0].clone(), 1, "one"))
        .unwrap();
    plugins
        .load(&two, observer_named(&f, logs[1].clone(), 1, "two"))
        .unwrap();
    let mut runtime =
        DerivedRuntime::build("two", plugins.snapshots(), Arc::new(LocalExecutor)).unwrap();
    runtime.start().unwrap();
    assert_eq!(runtime.drain(1).len(), 1);
    assert_eq!(logs[0].lock().unwrap().len(), 1);
    assert!(logs[1].lock().unwrap().is_empty());
    assert_eq!(runtime.drain(1).len(), 1);
    for v in 10..15 {
        f.put(&f.a, v);
        f.put(&f.b, v);
    }
    for _ in 0..2 {
        assert_eq!(runtime.drain(1).len(), 1);
    }
    assert_eq!(logs[0].lock().unwrap().len(), 2);
    assert_eq!(logs[1].lock().unwrap().len(), 2);
    assert_eq!(runtime.drain(2).len(), 2);
    assert!(runtime.drain(4).is_empty());
    assert_eq!(*logs[0].lock().unwrap(), vec![8, 28, 28]);
    assert_eq!(*logs[1].lock().unwrap(), vec![8, 28, 28]);
    drop(runtime);
    drop(plugins);
    f.put(&f.a, 100);
    assert_eq!(logs[0].lock().unwrap().len(), 3);
    assert_eq!(logs[1].lock().unwrap().len(), 3);
}

#[test]
fn binding_uses_final_engine_not_temporary_or_discarded_deployments() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (a, b) = (f.a.clone(), f.b.clone());
    let specs = Arc::new(Mutex::new(Vec::new()));
    let saved = specs.clone();
    let mut runtime = build(
        demand(move |g| {
            let mut discarded = GraphBuilder::new("derived");
            let fake = discarded
                .derive("sum", (&a, &b), |a, b| a + b)
                .map_err(|e| e.to_string())?;
            let fake = discarded
                .query_spec("query", &fake)
                .map_err(|e| e.to_string())?;
            let _temporary =
                Deployment::new(discarded.compile().map_err(|e| e.to_string())?).build()?;
            let sum = g
                .derive("sum", (&a, &b), |a, b| a + b)
                .map_err(|e| e.to_string())?;
            let real = g.query_spec("query", &sum).map_err(|e| e.to_string())?;
            let compiled = g.clone().compile().map_err(|e| e.to_string())?;
            // Both temporary deployments share the real stateless actions. Neither
            // should add registration counts or affect final deployment membership.
            let _first = Deployment::new(compiled.clone()).build()?;
            let _second = Deployment::new(compiled).build()?;
            *saved.lock().unwrap() = vec![fake, real];
            Ok(())
        }),
        Arc::new(LocalExecutor),
    );
    let specs = specs.lock().unwrap();
    assert!(matches!(
        runtime.bind_query(&specs[0]),
        Err(Error::WrongGeneration)
    ));
    runtime.start().unwrap();
    assert_eq!(runtime.bind_query(&specs[1]).unwrap().query().unwrap(), 8);
}

#[test]
fn failed_and_nested_builds_have_no_ambient_membership_state() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (a, b) = (f.a.clone(), f.b.clone());
    let first = Arc::new(AtomicBool::new(true));
    let saved = Arc::new(Mutex::new(Vec::new()));
    let specs = saved.clone();
    let plugin = demand(move |g| {
        let _nested = build(demand(|_| Ok(())), Arc::new(LocalExecutor));
        let sum = g
            .derive("sum", (&a, &b), |a, b| a + b)
            .map_err(|e| e.to_string())?;
        specs
            .lock()
            .unwrap()
            .push(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
        let _temporary =
            Deployment::new(g.clone().compile().map_err(|e| e.to_string())?).build()?;
        if first.swap(false, Ordering::SeqCst) {
            return Err("composition failed after temporary deployment".into());
        }
        Ok(())
    });
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("retry", |p: Plugin| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, plugin).unwrap();
    assert!(DerivedRuntime::build("retry", plugins.snapshots(), Arc::new(LocalExecutor)).is_err());
    let mut runtime =
        DerivedRuntime::build("retry", plugins.snapshots(), Arc::new(LocalExecutor)).unwrap();
    let specs = saved.lock().unwrap();
    assert!(matches!(
        runtime.bind_query(&specs[0]),
        Err(Error::WrongGeneration)
    ));
    runtime.start().unwrap();
    assert_eq!(runtime.bind_query(&specs[1]).unwrap().query().unwrap(), 8);
}

#[test]
fn shared_actions_in_two_module_instances_are_ambiguous() {
    let f = Fixture::new(true);
    let (a, b) = (f.a.clone(), f.b.clone());
    let saved = Arc::new(Mutex::new(None));
    let specs = saved.clone();
    let runtime = build(
        demand(move |g| {
            let mut child = GraphBuilder::new("child");
            let sum = child
                .derive("sum", (&a, &b), |a, b| a + b)
                .map_err(|e| e.to_string())?;
            *specs.lock().unwrap() =
                Some(child.query_spec("query", &sum).map_err(|e| e.to_string())?);
            let module = child.module();
            g.instance("one", &module)?;
            g.instance("two", &module)?;
            Ok(())
        }),
        Arc::new(LocalExecutor),
    );
    assert!(matches!(
        runtime.bind_query(saved.lock().unwrap().as_ref().unwrap()),
        Err(Error::WrongGeneration)
    ));
}

struct OffloadExecutor;
impl Executor for OffloadExecutor {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        std::thread::scope(|scope| scope.spawn(move || action.execute(ctx)).join().unwrap())
    }
}

#[test]
fn demand_and_observer_nodes_cannot_query_another_runtime_even_when_offloaded() {
    for offload in [false, true] {
        let reads = Arc::new(AtomicUsize::new(0));
        let target = Fixture::with_backend(
            false,
            ReadProbe {
                memory: MemoryBackend::default(),
                reads: reads.clone(),
                denied: Arc::new(AtomicBool::new(false)),
            },
        );
        target.put(&target.a, 3);
        target.put(&target.b, 5);
        let (plugin, spec) = sum_plugin(&target);
        let mut target_runtime = build(plugin, Arc::new(LocalExecutor));
        target_runtime.start().unwrap();
        let target_spec = spec.lock().unwrap().clone().unwrap();
        let target_runtime = Arc::new(Mutex::new(target_runtime));
        let nested_target = target_runtime.clone();
        let nested_spec = target_spec.clone();
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (a, b) = (f.a.clone(), f.b.clone());
        let request = Arc::new(Mutex::new(None::<Observation>));
        let activate = request.clone();
        let saved = Arc::new(Mutex::new(None));
        let specs = saved.clone();
        let rejected = Arc::new(AtomicUsize::new(0));
        let calls = rejected.clone();
        let executor: Arc<dyn Executor> = if offload {
            Arc::new(OffloadExecutor)
        } else {
            Arc::new(LocalExecutor)
        };
        let mut runtime = build(
            Plugin {
                routes: vec!["sum.sources.refresh".into()],
                compose: Arc::new(move |g| {
                    let sum = g
                        .derive("sum", (&a, &b), |a, b| a + b)
                        .map_err(|e| e.to_string())?;
                    *request.lock().unwrap() = Some(sum.observation(1));
                    *specs.lock().unwrap() =
                        Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
                    let target = nested_target.clone();
                    let spec = nested_spec.clone();
                    let calls = calls.clone();
                    let probe = g
                        .node("probe", move |_: i64| {
                            // The independent target has no recursive consumer, so this
                            // regression would perform a real nested read without the guard.
                            assert!(matches!(
                                target.lock().unwrap().bind_query(&spec).unwrap().query(),
                                Err(Error::NestedQuery)
                            ));
                            calls.fetch_add(1, Ordering::SeqCst);
                        })
                        .map_err(|e| e.to_string())?;
                    g.connect(sum.output(), probe.inlet())?;
                    Ok(())
                }),
                activate: Some(Arc::new(move || {
                    activate
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .activate()
                        .map_err(|e| e.to_string())
                })),
            },
            executor,
        );
        runtime.start().unwrap();
        let spec = saved.lock().unwrap().clone().unwrap();
        assert_eq!(runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
        assert!(runtime.drain(1).remove(0).result.unwrap().success()); // observer bootstrap
        f.put(&f.b, 6);
        assert!(runtime.drain(1).remove(0).result.unwrap().success());
        assert_eq!(rejected.load(Ordering::SeqCst), 3);
        assert_eq!(reads.load(Ordering::SeqCst), 0); // rejected before Kernel::fire
        assert_eq!(
            target_runtime
                .lock()
                .unwrap()
                .bind_query(&target_spec)
                .unwrap()
                .query()
                .unwrap(),
            8
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn demand_node_cannot_recursively_query_its_own_runtime() {
    thread_local! { static SELF_QUERY: std::cell::RefCell<Option<Box<dyn Fn() -> bool>>> = const { std::cell::RefCell::new(None) }; }
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            SELF_QUERY.with(|slot| *slot.borrow_mut() = None);
        }
    }
    let _clear = Clear;
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (a, b) = (f.a.clone(), f.b.clone());
    let saved = Arc::new(Mutex::new(None));
    let specs = saved.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = attempts.clone();
    let mut runtime = build(
        demand(move |g| {
            let sum = g
                .derive("sum", (&a, &b), |a, b| a + b)
                .map_err(|e| e.to_string())?;
            let spec = g.query_spec("query", &sum).map_err(|e| e.to_string())?;
            *specs.lock().unwrap() = Some(spec.clone());
            let calls = calls.clone();
            let node = g
                .node("probe", move |_: i64| {
                    // Bound recursion even on a regressed implementation, rather than
                    // overflowing the stack or deadlocking the test process.
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        assert!(SELF_QUERY.with(|slot| slot.borrow().as_ref().unwrap()()));
                    }
                })
                .map_err(|e| e.to_string())?;
            g.connect(sum.output(), node.inlet())?;
            Ok(())
        }),
        Arc::new(LocalExecutor),
    );
    runtime.start().unwrap();
    let runtime = std::rc::Rc::new(runtime);
    let spec = saved.lock().unwrap().clone().unwrap();
    let owner = runtime.clone();
    let nested_spec = spec.clone();
    SELF_QUERY.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            matches!(
                owner.bind_query(&nested_spec).unwrap().query(),
                Err(Error::NestedQuery)
            )
        }))
    });
    assert_eq!(runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

struct DispatchFailure {
    first: AtomicBool,
    panic: bool,
    calls: AtomicUsize,
}
impl Executor for DispatchFailure {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.first.swap(false, Ordering::SeqCst) {
            assert!(!self.panic, "executor panic fixture");
            return Err("executor failure fixture".into());
        }
        action.execute(ctx)
    }
}
#[test]
fn dispatch_guard_restores_after_executor_error_and_panic_and_delegates_once() {
    for panic in [false, true] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (plugin, saved) = sum_plugin(&f);
        let executor = Arc::new(DispatchFailure {
            first: AtomicBool::new(true),
            panic,
            calls: AtomicUsize::new(0),
        });
        let mut runtime = build(plugin, executor.clone());
        runtime.start().unwrap();
        let spec = saved.lock().unwrap().clone().unwrap();
        let query = runtime.bind_query(&spec).unwrap();
        assert!(matches!(query.query(), Err(Error::Wave(_))));
        assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        assert_eq!(query.query().unwrap(), 8);
        assert_eq!(executor.calls.load(Ordering::SeqCst), 4);
    }
}

#[test]
fn dispatch_guard_restores_after_node_panic() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let (a, b) = (f.a.clone(), f.b.clone());
    let first = Arc::new(AtomicBool::new(true));
    let saved = Arc::new(Mutex::new(None));
    let specs = saved.clone();
    let mut runtime = build(
        demand(move |g| {
            let first = first.clone();
            let sum = g
                .derive("sum", (&a, &b), move |a, b| {
                    assert!(!first.swap(false, Ordering::SeqCst), "node panic fixture");
                    a + b
                })
                .map_err(|e| e.to_string())?;
            *specs.lock().unwrap() = Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
            Ok(())
        }),
        Arc::new(LocalExecutor),
    );
    runtime.start().unwrap();
    let spec = saved.lock().unwrap().clone().unwrap();
    let query = runtime.bind_query(&spec).unwrap();
    assert!(matches!(query.query(), Err(Error::Wave(_))));
    assert_eq!(query.query().unwrap(), 8);
}

#[test]
fn an_authored_module_can_be_deliberately_redeployed_and_each_query_pins_its_runtime() {
    let f = Fixture::new(true);
    f.put(&f.a, 3);
    f.put(&f.b, 5);
    let mut child = GraphBuilder::new("authored");
    let sum = child.derive("sum", (&f.a, &f.b), |a, b| a + b).unwrap();
    let spec = child.query_spec("query", &sum).unwrap();
    let route = sum.refresh_route().to_owned();
    let module = child.module();
    let plugin = demand(move |g| {
        let instance = g.instance("authored", &module)?;
        let input = g.expose_input(&route, instance.local_input::<RefreshCause>(&route)?)?;
        g.trigger(&route, [input])?;
        Ok(())
    });
    let mut one = build(plugin.clone(), Arc::new(LocalExecutor));
    let mut two = build(plugin, Arc::new(LocalExecutor));
    one.start().unwrap();
    two.start().unwrap();
    let first = one.bind_query(&spec).unwrap();
    let second = two.bind_query(&spec).unwrap();
    assert_eq!(first.query().unwrap(), 8);
    f.put(&f.b, 6);
    assert_eq!(second.query().unwrap(), 9);
    assert_eq!(first.query().unwrap(), 9);
}

#[test]
fn query_binding_preflights_missing_misdirected_and_multi_target_module_routes() {
    for mode in ["missing", "misdirected", "wrong_type", "multi", "valid"] {
        let reads = Arc::new(AtomicUsize::new(0));
        let f = Fixture::with_backend(
            false,
            ReadProbe {
                memory: MemoryBackend::default(),
                reads: reads.clone(),
                denied: Arc::new(AtomicBool::new(false)),
            },
        );
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let mut child = GraphBuilder::new("authored");
        let sum = child.derive("sum", (&f.a, &f.b), |a, b| a + b).unwrap();
        let spec = child.query_spec("query", &sum).unwrap();
        let route = sum.refresh_route().to_owned();
        let expected_route = route.clone();
        let module = child.module();
        let effects = Arc::new(AtomicUsize::new(0));
        let calls = effects.clone();
        let mut runtime = build(
            demand(move |g| {
                let instance = g.instance("authored", &module)?;
                if mode == "missing" {
                    return Ok(());
                }
                let calls = calls.clone();
                let unrelated = if mode == "wrong_type" {
                    let node = g
                        .node("unrelated", move |_: String| {
                            calls.fetch_add(1, Ordering::SeqCst);
                        })
                        .map_err(|e| e.to_string())?;
                    g.expose_input("unrelated", node.inlet())?
                } else {
                    let node = g
                        .node("unrelated", move |_: RefreshCause| {
                            calls.fetch_add(1, Ordering::SeqCst);
                        })
                        .map_err(|e| e.to_string())?;
                    g.expose_input("unrelated", node.inlet())?
                };
                let root = g.expose_input("root", instance.local_input::<RefreshCause>(&route)?)?;
                match mode {
                    "misdirected" | "wrong_type" => {
                        g.trigger(&route, [unrelated])?;
                    }
                    "multi" => {
                        g.trigger(&route, [root, unrelated])?;
                    }
                    "valid" => {
                        g.trigger(&route, [root])?;
                    }
                    _ => unreachable!(),
                }
                Ok(())
            }),
            Arc::new(LocalExecutor),
        );
        if mode == "valid" {
            runtime.bind_query(&spec).unwrap();
        } else {
            assert!(
                matches!(runtime.bind_query(&spec), Err(Error::InvalidRefreshRoute(route)) if route == expected_route),
                "mode={mode}"
            );
        }
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        runtime.start().unwrap();
        if mode == "valid" {
            assert_eq!(runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
        } else {
            assert!(
                matches!(
                    runtime.bind_query(&spec),
                    Err(Error::InvalidRefreshRoute(_))
                ),
                "mode={mode}"
            );
            assert_eq!(reads.load(Ordering::SeqCst), 0);
        }
        assert_eq!(effects.load(Ordering::SeqCst), 0);
    }
}

struct LifetimeBackend {
    memory: MemoryBackend,
    _lease: Arc<()>,
    drops: Arc<AtomicUsize>,
}
impl Backend for LifetimeBackend {
    fn typed_kv(&mut self) -> Option<&mut dyn TypedKv> {
        Some(&mut self.memory)
    }
    fn observation(&self) -> phenix_storage::Observation {
        self.memory.observation()
    }
}
impl Drop for LifetimeBackend {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn backend_lives_until_runtime_and_all_saved_specs_are_dropped_in_either_order() {
    for spec_first in [false, true] {
        let lease = Arc::new(());
        let weak = Arc::downgrade(&lease);
        let drops = Arc::new(AtomicUsize::new(0));
        let f = Fixture::with_backend(
            true,
            LifetimeBackend {
                memory: MemoryBackend::default(),
                _lease: lease,
                drops: drops.clone(),
            },
        );
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (a, b) = (f.a.clone(), f.b.clone());
        let saved = Arc::new(Mutex::new(None));
        let specs = saved.clone();
        let observation = Arc::new(Mutex::new(None::<Observation>));
        let activate = observation.clone();
        let mut runtime = build(
            Plugin {
                routes: vec!["sum.sources.refresh".into()],
                compose: Arc::new(move |g| {
                    let sum = g
                        .derive("sum", (&a, &b), |a, b| a + b)
                        .map_err(|e| e.to_string())?;
                    *observation.lock().unwrap() = Some(sum.observation(1));
                    *specs.lock().unwrap() =
                        Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
                    Ok(())
                }),
                activate: Some(Arc::new(move || {
                    activate
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .activate()
                        .map_err(|e| e.to_string())
                })),
            },
            Arc::new(LocalExecutor),
        );
        let spec = saved.lock().unwrap().take().unwrap();
        let second_spec = spec.clone();
        runtime.start().unwrap();
        assert!(runtime.drain(1).remove(0).result.unwrap().success());
        assert_eq!(runtime.bind_query(&spec).unwrap().query().unwrap(), 8);
        drop(f); // Remove the original Registry, Session and cell owners.
        assert!(weak.upgrade().is_some());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        if spec_first {
            drop(spec);
            drop(second_spec);
            assert!(weak.upgrade().is_some());
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            drop(runtime);
        } else {
            drop(runtime);
            assert!(weak.upgrade().is_some());
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            drop(spec);
            assert!(weak.upgrade().is_some());
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            drop(second_spec);
        }
        assert!(weak.upgrade().is_none());
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

struct RepublishElsewhere {
    root: bool,
    saved: Mutex<Option<Vec<Emission>>>,
    intercepted: AtomicUsize,
}
impl Executor for RepublishElsewhere {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        let selected = if self.root {
            ctx.shared::<RefreshCause>(0).is_ok()
        } else {
            ctx.shared::<i64>(0).is_ok()
        };
        let mut values = action.execute(ctx)?;
        if selected {
            let mut saved = self.saved.lock().unwrap();
            self.intercepted.fetch_add(1, Ordering::SeqCst);
            if self.root {
                let receipt = values.pop().unwrap();
                if let Some(original) = &*saved {
                    values.extend(original.clone());
                } else {
                    *saved = Some(vec![receipt]);
                }
            } else if let Some(original) = &*saved {
                values = original.clone();
            } else {
                *saved = Some(values);
                values = vec![];
            }
        }
        Ok(values)
    }
}
#[test]
fn same_wave_payloads_republished_at_other_compatible_endpoints_are_rejected() {
    for root in [false, true] {
        let f = Fixture::new(true);
        f.put(&f.a, 3);
        f.put(&f.b, 5);
        let (a, b) = (f.a.clone(), f.b.clone());
        let saved = Arc::new(Mutex::new(None));
        let specs = saved.clone();
        let executor = Arc::new(RepublishElsewhere {
            root,
            saved: Mutex::new(None),
            intercepted: AtomicUsize::new(0),
        });
        let mut runtime = build(
            demand(move |g| {
                let sum = g
                    .derive("sum", (&a, &b), |a, b| a + b)
                    .map_err(|e| e.to_string())?;
                *specs.lock().unwrap() =
                    Some(g.query_spec("query", &sum).map_err(|e| e.to_string())?);
                if root {
                    let other = g
                        .derive("other", (&a, &b), |a, b| a + b)
                        .map_err(|e| e.to_string())?;
                    let gate = g
                        .node("gate", |_: (Number, Number)| RefreshCause::Demand)
                        .map_err(|e| e.to_string())?;
                    g.connect(
                        g.local_output::<(Number, Number)>("sum.sources", "value")?,
                        gate.inlet(),
                    )?;
                    g.connect(
                        gate.output(),
                        g.local_input::<RefreshCause>("other.sources", "run", "value")?,
                    )?;
                    // Keep the second computation ordinary, without a second tap.
                    let _ = other;
                } else {
                    g.query_spec("decoy", &sum).map_err(|e| e.to_string())?;
                }
                Ok(())
            }),
            executor.clone(),
        );
        runtime.start().unwrap();
        let spec = saved.lock().unwrap().clone().unwrap();
        let result = runtime.bind_query(&spec).unwrap().query();
        if root {
            assert!(
                matches!(result, Err(Error::InvalidPublication)),
                "{result:?}"
            );
        } else {
            assert!(
                matches!(result, Err(Error::PublicationCount(0))),
                "{result:?}"
            );
        }
        assert_eq!(executor.intercepted.load(Ordering::SeqCst), 2);
    }
}
