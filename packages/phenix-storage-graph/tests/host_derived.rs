#[allow(dead_code)]
#[path = "../examples/derived.rs"]
mod example;

use example::{A, Amount, B, TempRoot};
use phenix_graph::{Emission, Executor, InvocationCtx, LocalExecutor, NodeAction};
use phenix_kernel::host::{Binding, LifecycleError};
use phenix_kernel::{BuildOptions, Contribution, Key};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::scoped::Storage;
use phenix_storage_graph::derived::{
    DerivedGeneration, DerivedHost, Error, GraphDeriveExt, QuerySpec,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct Queries(QuerySpec<i64>);

struct Store {
    queries: Queries,
}
impl Store {
    fn bind(binding: &Binding<'_>, key: &Key) -> Result<Self, phenix_kernel::CallbackError> {
        let queries = binding
            .export::<Queries>(key)
            .ok_or("missing Queries")?
            .clone();
        queries.0.validate_binding(binding)?;
        Ok(Self { queries })
    }
    fn query(&self, generation: &DerivedGeneration) -> Result<i64, Error> {
        generation.query(&self.queries.0)
    }
}

struct Reader {
    storage: Arc<Storage>,
    starts: Arc<AtomicUsize>,
    admissions: Arc<AtomicUsize>,
    nested: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl PluginInstance for Reader {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let storage = self.storage.clone();
        let starts = self.starts.clone();
        let admissions = self.admissions.clone();
        let nested = self.nested.clone();
        Contribution::new("reader", move |context| {
                let (a, b) = storage.bind((A.read(), B.read()))?;
                let graph = &mut *context.graph;
                let sum = graph.derive("sum", (&a, &b), |a: Amount, b: Amount| a.0 + b.0)?;
                let admissions = admissions.clone();
                let nested = nested.clone();
                let consumer = graph.node("consumer", move |_: i64| {
                    admissions.fetch_add(1, Ordering::SeqCst);
                    if let Some(nested) = &nested {
                        nested();
                    }
                })?;
                graph.connect(sum.output(), consumer.inlet())?;
                let queries = Queries(graph.query_spec("query", &sum)?);
                let observation = sum.observation(16);
                context.export(queries)?;
                let starts = starts.clone();
                context.activate(move || {
                    starts.fetch_add(1, Ordering::SeqCst);
                    observation.activate().map_err(Into::into)
                })
            })
.route(vec!["sum.sources.refresh".into()])
.register(registrar)
    }
}

#[test]
fn host_exports_bind_before_start_and_pin_exact_old_and_new_snapshots() {
    let root = TempRoot::new().unwrap();
    let mut first = example::build(root.path(), "first").unwrap();
    let mut second = example::build(root.path(), "second").unwrap();
    first.runtime.start().unwrap();
    second.runtime.start().unwrap();
    first.write(true, 3).unwrap();
    first.write(false, 5).unwrap();
    second.write(true, 10).unwrap();
    second.write(false, 20).unwrap();
    let starts = Arc::new(AtomicUsize::new(0));
    let admissions = Arc::new(AtomicUsize::new(0));
    let reader = |storage| Reader {
        storage,
        starts: starts.clone(),
        admissions: admissions.clone(),
        nested: None,
    };
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("derive", |p: Reader| Ok(p)).unwrap();
    let mut host = DerivedHost::new(catalog.instance());
    let key = Key::new("derive", "reader");
    let mut old_store = None;
    let candidate = host
        .prepare(
            vec![
                host.plugins()
                    .prepare_load(&kind, reader(first.readers[0].clone()))
                    .unwrap(),
            ],
            BuildOptions::default(),
            Arc::new(LocalExecutor),
            "old",
            |binding| {
                assert_eq!(starts.load(Ordering::SeqCst), 0);
                old_store = Some(Store::bind(binding, &key)?);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(admissions.load(Ordering::SeqCst), 0);
    let old = host.publish(candidate).unwrap();
    let old_store = old_store.unwrap();
    assert_eq!(old_store.query(&old).unwrap(), 8);
    assert_eq!(old.drain(16).len(), 1); // bootstrap, once

    let failed = host.prepare(
        vec![
            host.plugins()
                .prepare_reload(&kind, reader(second.readers[0].clone()))
                .unwrap(),
        ],
        BuildOptions::default(),
        Arc::new(LocalExecutor),
        "failed",
        |binding| Store::bind(binding, &Key::new("missing", "reader")).map(|_| ()),
    );
    assert!(matches!(failed, Err(LifecycleError::Binding(_))));
    assert!(std::ptr::eq(&*host.active().unwrap(), &*old));
    assert_eq!(host.plugins().snapshot(&kind).unwrap().revision(), 1);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let before = admissions.load(Ordering::SeqCst);
    first.write(false, 7).unwrap();
    // Both cells share a dataset: its notice reaches each source subscription.
    assert_eq!(old.drain(16).len(), 2);
    assert_eq!(admissions.load(Ordering::SeqCst), before + 2);
    assert!(old.drain(16).is_empty());

    let mut new_store = None;
    let candidate = host
        .prepare(
            vec![
                host.plugins()
                    .prepare_reload(&kind, reader(second.readers[0].clone()))
                    .unwrap(),
            ],
            BuildOptions::default(),
            Arc::new(LocalExecutor),
            "new",
            |binding| {
                assert!(matches!(
                    old_store.queries.0.validate_binding(binding),
                    Err(Error::WrongGeneration)
                ));
                new_store = Some(Store::bind(binding, &key)?);
                Ok(())
            },
        )
        .unwrap();
    let new = host.publish(candidate).unwrap();
    let new_store = new_store.unwrap();
    // Retrieve the generation-owned export through the public scoped view too.
    let retrieved = new.with_kernel(|kernel| Store {
        queries: kernel.export::<Queries>(&key).unwrap().clone(),
    });
    assert_eq!(retrieved.query(&new).unwrap(), 30);
    assert_eq!(old_store.query(&old).unwrap(), 10);
    assert_eq!(new_store.query(&new).unwrap(), 30);
    assert!(matches!(old_store.query(&new), Err(Error::WrongGeneration)));
    assert!(matches!(new_store.query(&old), Err(Error::WrongGeneration)));
    assert!(old.drain(16).is_empty());
    assert_eq!(new.drain(16).len(), 1);
    assert!(new.drain(16).is_empty());
    assert_eq!(starts.load(Ordering::SeqCst), 2);
}

#[test]
fn derived_host_passes_execution_limits_to_queries_and_producers() {
    let root = TempRoot::new().unwrap();
    let mut demo = example::build(root.path(), "budget").unwrap();
    demo.runtime.start().unwrap();
    demo.write(true, 3).unwrap();
    demo.write(false, 5).unwrap();
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("derive", |p: Reader| Ok(p)).unwrap();
    let mut host = DerivedHost::new(catalog.instance());
    let stage = host
        .plugins()
        .prepare_load(
            &kind,
            Reader {
                storage: demo.readers[0].clone(),
                starts: Arc::default(),
                admissions: Arc::default(),
                nested: None,
            },
        )
        .unwrap();
    let mut stages = vec![stage];
    for limit in [0, phenix_graph::DEFAULT_MAX_ACTIVATIONS] {
        let mut store = None;
        let candidate = host
            .prepare(
                std::mem::take(&mut stages),
                BuildOptions {
                    imports: Default::default(),
                    execution: phenix_graph::ExecutionLimits {
                        max_activations: limit,
                    },
                },
                Arc::new(LocalExecutor),
                "budget",
                |binding| {
                    store = Some(Store::bind(binding, &Key::new("derive", "reader"))?);
                    Ok(())
                },
            )
            .unwrap();
        let generation = host.publish(candidate).unwrap();
        let result = store.unwrap().query(&generation);
        if limit == 0 {
            assert!(matches!(result, Err(Error::Wave(_))));
        } else {
            assert!(result.is_ok(), "{result:?}");
        }
        let events = generation.drain(1);
        assert_eq!(events.len(), 1);
        let report = events[0].result.as_ref().unwrap();
        assert_eq!(report.budget_exhausted(), limit == 0);
        assert_eq!(report.success(), limit != 0);
    }
}

struct Offload;
impl Executor for Offload {
    fn dispatch(
        &self,
        action: &dyn NodeAction,
        ctx: InvocationCtx,
    ) -> Result<Vec<Emission>, String> {
        std::thread::scope(|scope| scope.spawn(move || action.execute(ctx)).join().unwrap())
    }
}

#[test]
fn host_dispatch_rejects_cross_runtime_queries_on_local_and_worker_threads() {
    for executor in [
        Arc::new(LocalExecutor) as Arc<dyn Executor>,
        Arc::new(Offload),
    ] {
        let root = TempRoot::new().unwrap();
        let mut demo = example::build(root.path(), "nested").unwrap();
        demo.runtime.start().unwrap();
        demo.write(true, 3).unwrap();
        demo.write(false, 5).unwrap();
        let queries = demo
            .runtime
            .kernel()
            .export::<example::reader::Queries>(&Key::new("derive", "derive"))
            .unwrap()
            .clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let storage = demo.readers[0].clone();
        let demo = Arc::new(Mutex::new(demo));
        let target = demo.clone();
        let attempts = calls.clone();
        let mut catalog = PluginCatalog::new();
        let kind = catalog.register("derive", |p: Reader| Ok(p)).unwrap();
        let mut host = DerivedHost::new(catalog.instance());
        let mut target_host = DerivedHost::new(catalog.instance());
        let mut target_store = None;
        let target_candidate = target_host
            .prepare(
                vec![
                    target_host
                        .plugins()
                        .prepare_load(
                            &kind,
                            Reader {
                                storage: storage.clone(),
                                starts: Arc::default(),
                                admissions: Arc::default(),
                                nested: None,
                            },
                        )
                        .unwrap(),
                ],
                BuildOptions::default(),
                executor.clone(),
                "target",
                |binding| {
                    target_store = Some(Store::bind(binding, &Key::new("derive", "reader"))?);
                    Ok(())
                },
            )
            .unwrap();
        let target_generation = target_host.publish(target_candidate).unwrap();
        let target_store = target_store.unwrap();
        assert_eq!(target_store.query(&target_generation).unwrap(), 8);
        let nested_generation = target_generation.clone();
        let nested_store = Store {
            queries: target_store.queries.clone(),
        };
        let mut store = None;
        let candidate = host
            .prepare(
                vec![
                    host.plugins()
                        .prepare_load(
                            &kind,
                            Reader {
                                storage,
                                starts: Arc::default(),
                                admissions: Arc::default(),
                                nested: Some(Arc::new(move || {
                                    assert!(matches!(
                                        nested_store.query(&nested_generation),
                                        Err(Error::NestedQuery)
                                    ));
                                    let target = target.lock().unwrap();
                                    assert!(matches!(
                                        target.runtime.bind_query(&queries.sum).unwrap().query(),
                                        Err(Error::NestedQuery)
                                    ));
                                    // Explicit low-level queries share the guard too.
                                    assert!(matches!(
                                        queries
                                            .sum
                                            .bind_engine(target.runtime.kernel().engine())
                                            .unwrap()
                                            .query(),
                                        Err(Error::NestedQuery)
                                    ));
                                    attempts.fetch_add(1, Ordering::SeqCst);
                                })),
                            },
                        )
                        .unwrap(),
                ],
                BuildOptions::default(),
                executor,
                "guarded",
                |binding| {
                    store = Some(Store::bind(binding, &Key::new("derive", "reader"))?);
                    Ok(())
                },
            )
            .unwrap();
        let generation = host.publish(candidate).unwrap();
        let store = store.unwrap();
        assert_eq!(store.query(&generation).unwrap(), 8);
        assert_eq!(generation.drain(16).len(), 1);
        demo.lock().unwrap().write(false, 6).unwrap();
        assert_eq!(generation.drain(16).len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(target_store.query(&target_generation).unwrap(), 9);
        // Guard restoration permits a subsequent ordinary host query.
        assert_eq!(demo.lock().unwrap().query_all(0).unwrap(), [9, -3, -27, 18]);
    }
}
