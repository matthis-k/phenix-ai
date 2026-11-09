//! Graph-native derived values across independently authorized plugins.
//! Run: cargo run -p phenix-storage-graph --example derived
//! Notifications are registry-local invalidations, not a durable/cross-process feed.
use phenix_graph::{LocalExecutor, Value};
use phenix_kernel::{Contribution, Key};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{
    Codec, Operation, Revisioned, Schema,
    scoped::{BindingTarget, CellSpec, Identity, Storage, Stored, XdgPaths},
    snapshot::{ReadConsistency, SnapshotReader},
};
use phenix_storage_graph::derived::{DerivedRuntime, GraphDeriveExt, QuerySpec};
use phenix_storage_plugin::{
    BackendPlacement, PreparedStorage, ResourceBinding, ResourceOffer, ResourceRequirements,
    StoragePolicy,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
type Slot<T> = Arc<OnceLock<T>>;
type Log<T> = Arc<Mutex<Vec<T>>>;

// Shared domain contract, not a built-in codec for arbitrary Rust integers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Amount(pub i64);
#[derive(Default)]
pub struct AmountCodec;
impl Stored for Amount {
    type Format = AmountCodec;
}
impl Codec for AmountCodec {
    type Value = Amount;
    fn schema(&self) -> Schema {
        Schema {
            name: "demo.amount".into(),
            revision: 1,
            structure: "i64".into(),
            codec: "big-endian".into(),
        }
    }
    fn encode(&self, value: &Amount) -> phenix_storage::Result<Vec<u8>> {
        Ok(value.0.to_be_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<Amount> {
        Ok(Amount(i64::from_be_bytes(bytes.try_into().map_err(
            |_| phenix_storage::Error::Codec("expected eight bytes".into()),
        )?)))
    }
}
pub const A: CellSpec<Amount> = CellSpec::project_state("demo.values/a");
pub const B: CellSpec<Amount> = CellSpec::project_state("demo.values/b");

fn offers(registrar: &mut Registrar, operations: &[Operation]) -> Result<(), Failure> {
    for (id, spec) in [("a", A), ("b", B)] {
        registrar.add(ResourceOffer::new(
            id,
            phenix_storage::scoped::ResourceRequest::new(
                spec.resource_spec(),
                operations.iter().copied(),
            ),
        ))?;
    }
    Ok(())
}

pub mod writer {
    use super::*;
    #[derive(Clone)]
    pub struct Update {
        pub a: bool,
        pub value: Amount,
    }
    pub struct Plugin {
        pub storage: Slot<Arc<Storage>>,
        pub writes: Log<Result<(), String>>,
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            offers(registrar, &[Operation::Write])?;
            let storage = self.storage.clone();
            let writes = self.writes.clone();
            Contribution::new("writer", move |context| {
                let graph = &mut *context.graph;
                let project = storage
                    .get()
                    .ok_or("writer storage not injected")?
                    .project()?;
                let a = project.state::<Amount>("demo.values/a")?;
                let b = project.state::<Amount>("demo.values/b")?;
                let writes = writes.clone();
                let node = graph.node("write", move |update: Update| {
                    let target = if update.a { &a } else { &b };
                    writes
                        .lock()
                        .unwrap()
                        .push(target.put("value", &update.value).map_err(text));
                })?;
                let input = graph.expose_input("update", node.inlet())?;
                graph.trigger("write", [input])?;
                Ok(())
            })
            .register(registrar)
        }
    }
}

pub mod reader {
    use super::*;
    #[derive(Clone)]
    pub struct Queries {
        pub sum: QuerySpec<i64>,
        pub diff: QuerySpec<i64>,
        pub both: QuerySpec<i64>,
        pub twice: QuerySpec<i64>,
    }
    #[derive(Default)]
    pub struct State {
        pub consumed: Log<i64>,
    }
    // Separate instances are separate plugins, each declaring and binding its own
    // logical sources. No handles or computed values are passed between plugins.
    pub struct Plugin {
        pub name: &'static str,
        pub storage: Slot<Arc<Storage>>,
        pub state: Arc<State>,
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            offers(registrar, &[Operation::Read, Operation::Subscribe])?;
            let name = self.name;
            let storage = self.storage.clone();
            let state = self.state.clone();
            Contribution::new(name, move |context| {
                let graph = &mut *context.graph;
                let (a, b) = storage
                    .get()
                    .ok_or("reader storage not injected")?
                    .bind((A.read(), B.read()))?;
                let group = graph.sources(&format!("{name}.values"), (&a, &b))?;
                let sum =
                    graph.derive_from(&format!("{name}.sum"), &group, |a: Amount, b: Amount| {
                        a.0 + b.0
                    })?;
                let diff = graph.derive_from(
                    &format!("{name}.diff"),
                    &group,
                    |a: Amount, b: Amount| a.0 - b.0,
                )?;
                let both = graph.derive(&format!("{name}.both"), (&sum, &diff), |sum, diff| {
                    sum * diff
                })?;
                let twice = graph.derive(&format!("{name}.twice"), (&sum,), |sum| sum * 2)?;
                let consumed = state.consumed.clone();
                let consumer = graph.node(format!("{name}.consumer"), move |value: i64| {
                    consumed.lock().unwrap().push(value);
                })?;
                graph.connect(twice.output(), consumer.inlet())?;
                let queries = Queries {
                    sum: graph.query_spec(&format!("{name}.qsum"), &sum)?,
                    diff: graph.query_spec(&format!("{name}.qdiff"), &diff)?,
                    both: graph.query_spec(&format!("{name}.qboth"), &both)?,
                    twice: graph.query_spec(&format!("{name}.qtwice"), &twice)?,
                };
                let observation = group.observation(16);
                context.export(queries)?;
                context.activate(move || observation.activate().map_err(Into::into))?;
                Ok(())
            })
            .route(vec![format!("{name}.values.refresh")])
            .register(registrar)
        }
    }
}

// Host decisions are fixed independently of every ResourceOffer.
pub fn host_policy(root: &Path, identity: &Identity) -> Result<StoragePolicy, String> {
    let a = A.resource_spec().resolve(identity).map_err(text)?;
    let paths = XdgPaths::new(
        root.join("config"),
        root.join("data"),
        root.join("state"),
        root.join("cache"),
        None,
    )
    .map_err(text)?;
    let mut policy = StoragePolicy::default();
    policy.mounts.insert(
        "project".into(),
        BackendPlacement::Sqlite(paths.directory(&a).map_err(text)?.join("values.sqlite")),
    );
    for (dataset, spec) in [("a", A), ("b", B)] {
        let resource = spec.resource_spec().resolve(identity).map_err(text)?;
        policy.resources.insert(
            resource.clone(),
            ResourceBinding {
                target: BindingTarget::Dataset(
                    phenix_storage::Address::parse(&format!("store://project/{dataset}"))
                        .map_err(text)?,
                ),
                allowed: [Operation::Write, Operation::Read, Operation::Subscribe].into(),
            },
        );
        for (plugin, operations) in [
            ("writer", vec![Operation::Write]),
            ("derive", vec![Operation::Read, Operation::Subscribe]),
            ("subscriber", vec![Operation::Read, Operation::Subscribe]),
        ] {
            policy
                .plugin_access
                .entry(plugin.into())
                .or_default()
                .insert(resource.clone(), operations.into_iter().collect());
        }
    }
    Ok(policy)
}

pub struct Demo {
    pub runtime: DerivedRuntime,
    pub writer: Arc<Storage>,
    pub readers: [Arc<Storage>; 2],
    pub states: [Arc<reader::State>; 2],
    pub writes: Log<Result<(), String>>,
}

pub fn build(root: &Path, project: &str) -> Result<Demo, String> {
    build_with_policy(root, project, |_| {})
}

pub fn build_with_policy(
    root: &Path,
    project: &str,
    edit_policy: impl FnOnce(&mut StoragePolicy),
) -> Result<Demo, String> {
    let mut catalog = PluginCatalog::new();
    let writer_kind = catalog
        .register("writer", |p: writer::Plugin| Ok(p))
        .map_err(text)?;
    let derive_kind = catalog
        .register("derive", |p: reader::Plugin| Ok(p))
        .map_err(text)?;
    let subscriber_kind = catalog
        .register("subscriber", |p: reader::Plugin| Ok(p))
        .map_err(text)?;
    let slots: [Slot<Arc<Storage>>; 3] = std::array::from_fn(|_| Slot::default());
    let states: [Arc<reader::State>; 2] = std::array::from_fn(|_| Arc::default());
    let writes = Log::default();
    let mut plugins = catalog.instance();
    plugins
        .load(
            &writer_kind,
            writer::Plugin {
                storage: slots[0].clone(),
                writes: writes.clone(),
            },
        )
        .map_err(text)?;
    for (i, (kind, name)) in [(&derive_kind, "derive"), (&subscriber_kind, "subscriber")]
        .into_iter()
        .enumerate()
    {
        plugins
            .load(
                kind,
                reader::Plugin {
                    name,
                    storage: slots[i + 1].clone(),
                    state: states[i].clone(),
                },
            )
            .map_err(text)?;
    }
    // Load once, then plan and compose the exact same plugin snapshots.
    let snapshots: Vec<_> = plugins.snapshots().collect();
    let identity = Identity::user("demo")
        .map_err(text)?
        .project(project)
        .map_err(text)?;
    let requirements = ResourceRequirements::collect(snapshots.clone(), &identity).map_err(text)?;
    let mut policy = host_policy(root, &identity)?;
    edit_policy(&mut policy);
    let mut active = PreparedStorage::prepare(requirements, identity, policy)
        .map_err(text)?
        .activate()
        .map_err(text)?;
    let mut views = Vec::new();
    for (slot, name) in slots.iter().zip(["writer", "derive", "subscriber"]) {
        let view = Arc::new(active.views.remove(name).ok_or("missing storage view")?);
        slot.set(view.clone())
            .map_err(|_| "storage already injected")?;
        views.push(view);
    }
    let runtime = DerivedRuntime::build("derived-plugins", snapshots, Arc::new(LocalExecutor))
        .map_err(text)?;
    let [writer, derive, subscriber]: [_; 3] = views.try_into().map_err(|_| "three views")?;
    Ok(Demo {
        runtime,
        writer,
        readers: [derive, subscriber],
        states,
        writes,
    })
}

impl Demo {
    pub fn write(&self, a: bool, value: i64) -> Result<(), String> {
        let report = self.runtime.kernel().fire(
            "write",
            vec![Value::local(writer::Update {
                a,
                value: Amount(value),
            })],
        )?;
        if !report.success() {
            return Err(report.error_messages().join("; "));
        }
        self.writes
            .lock()
            .unwrap()
            .last()
            .ok_or("no write result")?
            .clone()
    }
    pub fn drain(&mut self) -> Result<usize, String> {
        let events = self.runtime.drain(64);
        let count = events.len();
        for event in events {
            let report = event.result.map_err(|e| format!("{e:?}"))?;
            if !report.success() {
                return Err(report.error_messages().join("; "));
            }
        }
        Ok(count)
    }
    pub fn values(&self) -> Result<[Revisioned<Amount>; 2], String> {
        let (a, b) = self.readers[0].bind((A.read(), B.read())).map_err(text)?;
        let snapshot = SnapshotReader::new(
            vec![a.source(), b.source()],
            ReadConsistency::AtomicWithinMount,
        )
        .map_err(text)?
        .read()
        .map_err(text)?;
        Ok([
            a.decode(&snapshot.entries()[0]).map_err(text)?,
            b.decode(&snapshot.entries()[1]).map_err(text)?,
        ])
    }
    pub fn query_all(&self, reader: usize) -> Result<[i64; 4], String> {
        let name = ["derive", "subscriber"][reader];
        let q = self
            .runtime
            .kernel()
            .export::<reader::Queries>(&Key::new(name, name))
            .ok_or("queries not composed")?;
        let mut values = [0; 4];
        for (value, spec) in values.iter_mut().zip([&q.sum, &q.diff, &q.both, &q.twice]) {
            *value = self
                .runtime
                .bind_query(spec)
                .map_err(text)?
                .query()
                .map_err(text)?;
        }
        Ok(values)
    }
}

/// No extra dependency: unique temporary directory, removed after all DB handles.
pub struct TempRoot(PathBuf);
impl TempRoot {
    pub fn new() -> Result<Self, String> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(text)?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("phenix-derived-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).map_err(text)?;
        Ok(Self(path))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn run() -> Result<(), String> {
    let root = TempRoot::new()?;
    let mut demo = build(root.path(), "project-one")?;
    demo.runtime.start().map_err(text)?;
    demo.write(true, 3)?;
    demo.write(false, 5)?;
    // Subscriptions precede the initial reads; queued invalidations may also rescan.
    assert!(demo.drain()? >= 2);
    assert_eq!(demo.query_all(0)?, [8, -2, -16, 16]);
    assert_eq!(demo.query_all(1)?, [8, -2, -16, 16]);
    demo.write(true, 4)?;
    assert!(demo.drain()? >= 2);
    assert_eq!(demo.query_all(0)?, [9, -1, -9, 18]);
    let saved = demo.values()?;
    drop(demo);
    let mut reopened = build(root.path(), "project-one")?;
    assert_eq!(reopened.values()?, saved);
    reopened.runtime.start().map_err(text)?;
    assert_eq!(reopened.query_all(0)?, [9, -1, -9, 18]);
    let mut other = build(root.path(), "project-two")?;
    other.runtime.start().map_err(text)?;
    other.write(true, 100)?;
    other.write(false, 20)?;
    assert_eq!(other.query_all(0)?, [120, 80, 9600, 240]);
    assert_eq!(reopened.values()?, saved);
    assert!(matches!(
        reopened.writer.bind(A.read()),
        Err(phenix_storage::scoped::Error::Storage(
            phenix_storage::Error::Denied
        ))
    ));
    assert!(matches!(
        reopened.readers[0]
            .project()
            .map_err(text)?
            .state::<Amount>("demo.values/a")
            .map_err(text)?
            .put("value", &Amount(99)),
        Err(phenix_storage::Error::Denied)
    ));
    println!(
        "sum=9 diff=-1 both=-9 twice=18; independent subscriptions, project isolation, persisted revisions and denied grants verified"
    );
    Ok(())
}

#[allow(dead_code)]
fn main() -> Result<(), String> {
    run()
}
