#![allow(dead_code)]
#[path = "../../examples/derived.rs"]
pub mod example;
#[allow(unused_imports)]
pub use example::{A, Amount as Number, AmountCodec as Format, B, capacity, patch, sink};
use phenix_graph::{
    compose::{Footprint, NetworkPatch},
    network::NetworkBuilder,
    process::{LocalProcessDispatcher, ProcessDispatcher},
};
use phenix_kernel::{BuildOptions, CandidateArtifacts, CompositionPolicy, Contribution, Key};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{
    Address, Backend, Codec, Grant, MemoryBackend, Operation, Registry, Scope, Session,
    scoped::{Identity, ReadCell, ResourceCatalog},
};
use phenix_storage_graph::derived::*;
use std::sync::Arc;

pub struct Fixture {
    pub registry: Registry,
    pub session: Session,
    pub a: ReadCell<Number>,
    pub b: ReadCell<Number>,
}
impl Fixture {
    pub fn new(subscribe: bool) -> Self {
        Self::with_backend(subscribe, MemoryBackend::default())
    }
    pub fn with_backend(subscribe: bool, backend: impl Backend + 'static) -> Self {
        let registry = Registry::new();
        registry.mount("main", backend).unwrap();
        Self::from_registry(subscribe, registry)
    }
    pub fn from_registry(subscribe: bool, registry: Registry) -> Self {
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
        let mut operations = vec![Operation::Read];
        if subscribe {
            operations.push(Operation::Subscribe);
        }
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
    pub fn put(&self, cell: &ReadCell<Number>, value: i64) {
        self.session
            .put(cell.source().address(), "value", &Number(value), &Format)
            .unwrap();
    }
}

pub type Author =
    dyn Fn() -> Result<CandidateArtifacts, phenix_kernel::CallbackError> + Send + Sync;
#[derive(Clone)]
pub struct Plugin {
    pub author: Arc<Author>,
    pub grants: std::collections::BTreeSet<Footprint>,
    pub routes: Vec<String>,
}
impl PluginInstance for Plugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let author = self.author.clone();
        registrar.add(Contribution {
            id: "derive".into(),
            requires: vec![],
            routes: self.routes.clone(),
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| author())),
        })
    }
}
pub fn options(plugin: &Plugin) -> BuildOptions {
    let mut options = BuildOptions::default();
    options.composition.insert(
        Key::new("domain", "derive"),
        CompositionPolicy {
            grants: plugin.grants.clone(),
            priority: 0,
        },
    );
    options
}
pub fn build(plugin: Plugin, dispatcher: Arc<dyn ProcessDispatcher>) -> DerivedRuntime {
    let options = options(&plugin);
    configured(plugin, dispatcher, options)
}
pub fn configured(
    plugin: Plugin,
    dispatcher: Arc<dyn ProcessDispatcher>,
    options: BuildOptions,
) -> DerivedRuntime {
    let mut catalog = PluginCatalog::new();
    let kind = catalog.register("domain", |p: Plugin| Ok(p)).unwrap();
    let mut plugins = catalog.instance();
    plugins.load(&kind, plugin).unwrap();
    DerivedRuntime::build_with_options("derived", plugins.snapshots(), dispatcher, options).unwrap()
}
pub fn local(plugin: Plugin) -> DerivedRuntime {
    build(plugin, Arc::new(LocalProcessDispatcher))
}

/// Host-owned immutable authored declaration. Tests deliberately grant exactly
/// this fixture's writes, never a prepare callback's returned write inventory.
pub fn authored<T: Clone + Send + Sync + 'static>(
    graph: NetworkBuilder,
    spec: QuerySpec<T>,
    observation: Option<Observation>,
) -> Plugin {
    let patch = patch(graph.build().unwrap());
    let grants = patch.footprints();
    let routes = observation
        .as_ref()
        .map(|o| vec![o.route().into()])
        .unwrap_or_default();
    Plugin {
        grants,
        routes,
        author: Arc::new(move || {
            let mut artifacts = CandidateArtifacts {
                patch: Some(patch.clone()),
                ..Default::default()
            };
            artifacts.export(spec.clone())?;
            if let Some(observation) = observation.clone() {
                artifacts.activate(move || observation.activate().map_err(Into::into))?;
            }
            Ok(artifacts)
        }),
    }
}
pub fn sum(f: &Fixture, observe: bool) -> (Plugin, QuerySpec<i64>) {
    let mut graph = NetworkBuilder::new();
    let sum = graph
        .derive("sum", (&f.a, &f.b), |a: Number, b: Number| a.0 + b.0)
        .unwrap();
    let spec = graph.query_spec("query", &sum).unwrap();
    let observation = observe.then(|| sum.observation(1));
    (authored(graph, spec.clone(), observation), spec)
}
pub fn settle(runtime: &DerivedRuntime) {
    let outputs: Vec<_> = runtime
        .kernel()
        .compiled()
        .outputs()
        .keys()
        .cloned()
        .collect();
    runtime
        .kernel()
        .with_session(|session| {
            for _ in 0..128 {
                let report = session.run_until_idle(128);
                for output in &outputs {
                    while session.take_output(output).unwrap().is_some() {}
                }
                assert_ne!(
                    report.state,
                    phenix_graph::runtime::SessionState::Failed,
                    "{:?}",
                    session.failure()
                );
                if report.steps == 0 {
                    return;
                }
            }
            panic!("settlement budget exhausted");
        })
        .unwrap();
}
