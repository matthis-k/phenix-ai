#[allow(dead_code)]
#[path = "../examples/derived.rs"]
mod example;

use example::{A, Amount, B, TempRoot};
use phenix_graph::{Emission, Executor, InvocationCtx, LocalExecutor, NodeAction};
use phenix_kernel::host::{Generation, KernelHost, LifecycleError};
use phenix_kernel::{Contribution, ImportPolicy, Kernel, Key};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::scoped::Storage;
use phenix_storage_graph::derived::{DerivedRuntime, Error, GraphDeriveExt, QuerySpec};
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
    fn bind(kernel: &Kernel, key: &Key) -> Result<Self, String> {
        let queries = kernel
            .export::<Queries>(key)
            .ok_or("missing Queries")?
            .clone();
        queries
            .0
            .bind_engine(kernel.engine())
            .map_err(|e| e.to_string())?;
        Ok(Self { queries })
    }
    fn query(&self, generation: &Generation) -> Result<i64, Error> {
        generation.with_kernel(|kernel| self.queries.0.bind_engine(kernel.engine())?.query())
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
        registrar.add(Contribution {
            id: "reader".into(),
            requires: vec![],
            routes: vec!["sum.sources.refresh".into()],
            compose: Box::new(move |context| {
                let (a, b) = storage
                    .bind((A.read(), B.read()))
                    .map_err(|e| e.to_string())?;
                let graph = &mut *context.graph;
                let sum = graph
                    .derive("sum", (&a, &b), |a: Amount, b: Amount| a.0 + b.0)
                    .map_err(|e| e.to_string())?;
                let admissions = admissions.clone();
                let nested = nested.clone();
                let consumer = graph
                    .node("consumer", move |_: i64| {
                        admissions.fetch_add(1, Ordering::SeqCst);
                        if let Some(nested) = &nested {
                            nested();
                        }
                    })
                    .map_err(|e| e.to_string())?;
                graph.connect(sum.output(), consumer.inlet())?;
                let queries = Queries(graph.query_spec("query", &sum).map_err(|e| e.to_string())?);
                let observation = sum.observation(16);
                context.export(queries)?;
                let starts = starts.clone();
                context.activate(move || {
                    starts.fetch_add(1, Ordering::SeqCst);
                    observation.activate().map_err(|e| e.to_string())
                })
            }),
        })
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
    let mut host = KernelHost::new(catalog.instance());
    let key = Key::new("derive", "reader");
    let mut old_store = None;
    let candidate = host
        .prepare_with(
            vec![
                host.plugins()
                    .prepare_load(&kind, reader(first.readers[0].clone()))
                    .unwrap(),
            ],
            &ImportPolicy::default(),
            DerivedRuntime::guard_executor(Arc::new(LocalExecutor)),
            "old",
            |kernel| {
                assert_eq!(starts.load(Ordering::SeqCst), 0);
                old_store = Some(Store::bind(kernel, &key)?);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(admissions.load(Ordering::SeqCst), 0);
    let old = host.publish(candidate).unwrap();
    let old_store = old_store.unwrap();
    assert_eq!(old_store.query(&old).unwrap(), 8);
    assert_eq!(old.drain(16).len(), 1); // bootstrap, once

    let failed = host.prepare_with(
        vec![
            host.plugins()
                .prepare_reload(&kind, reader(second.readers[0].clone()))
                .unwrap(),
        ],
        &ImportPolicy::default(),
        DerivedRuntime::guard_executor(Arc::new(LocalExecutor)),
        "failed",
        |kernel| Store::bind(kernel, &Key::new("missing", "reader")).map(|_| ()),
    );
    assert!(matches!(failed, Err(LifecycleError::Binding(_))));
    assert!(Arc::ptr_eq(&host.active().unwrap(), &old));
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
        .prepare_with(
            vec![
                host.plugins()
                    .prepare_reload(&kind, reader(second.readers[0].clone()))
                    .unwrap(),
            ],
            &ImportPolicy::default(),
            DerivedRuntime::guard_executor(Arc::new(LocalExecutor)),
            "new",
            |kernel| {
                new_store = Some(Store::bind(kernel, &key)?);
                Ok(())
            },
        )
        .unwrap();
    let new = host.publish(candidate).unwrap();
    let new_store = new_store.unwrap();
    // Retrieve the generation-owned export through the public scoped view too.
    let retrieved = new.with_kernel(|kernel| Store::bind(kernel, &key)).unwrap();
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
        let mut host = KernelHost::new(catalog.instance());
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
                                    let target = target.lock().unwrap();
                                    assert!(matches!(
                                        target.runtime.bind_query(&queries.sum).unwrap().query(),
                                        Err(Error::NestedQuery)
                                    ));
                                    // The generation-compatible entry point shares the same guard.
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
                &ImportPolicy::default(),
                DerivedRuntime::guard_executor(executor),
                "guarded",
            )
            .unwrap();
        let generation = host.publish(candidate).unwrap();
        let store = generation
            .with_kernel(|kernel| Store::bind(kernel, &Key::new("derive", "reader")))
            .unwrap();
        assert_eq!(store.query(&generation).unwrap(), 8);
        assert_eq!(generation.drain(16).len(), 1);
        demo.lock().unwrap().write(false, 6).unwrap();
        assert_eq!(generation.drain(16).len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        // Guard restoration permits a subsequent ordinary host query.
        assert_eq!(demo.lock().unwrap().query_all(0).unwrap(), [9, -3, -27, 18]);
    }
}
