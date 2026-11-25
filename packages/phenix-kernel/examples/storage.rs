use phenix_graph::{
    Value, component::ComponentDef, compose::NetworkPatch, network::NetworkBuilder,
    process::LocalProcessDispatcher,
};
pub(crate) use scoped_storage::support;
#[allow(dead_code)]
#[path = "scoped_storage.rs"]
mod scoped_storage;
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, Contribution, InputEvent, Kernel, Key, Producer,
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::{
    Address, Codec, Grant, Operation, Registry, Schema, Scope, Session, Subscription,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

pub struct CounterCodec(pub u32);
impl Codec for CounterCodec {
    type Value = i64;
    fn schema(&self) -> Schema {
        Schema {
            name: "counter".into(),
            revision: self.0,
            structure: "i64".into(),
            codec: "le-i64".into(),
        }
    }
    fn encode(&self, value: &i64) -> phenix_storage::Result<Vec<u8>> {
        Ok(value.to_le_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<i64> {
        Ok(i64::from_le_bytes(bytes.try_into().map_err(|_| {
            phenix_storage::Error::Codec("expected eight bytes".into())
        })?))
    }
}

#[derive(Clone)]
pub struct Counter {
    pub session: Session,
    pub address: Address,
    pub sqlite: bool,
}
impl Counter {
    pub fn write(&self, value: i64) -> Result<(), String> {
        let result = if self.sqlite {
            self.session.transaction(&self.address, |sql| {
                sql.execute("INSERT INTO counters (name, value) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET value = excluded.value", &[&"counter", &value])?;
                Ok(())
            })
        } else {
            self.session
                .put(&self.address, "counter", &value, &CounterCodec(1))
        };
        result.map_err(|e| e.to_string())
    }
    pub fn read(&self) -> Result<Option<i64>, String> {
        let result = if self.sqlite {
            self.session.transaction(&self.address, |sql| {
                Ok(sql
                    .query(
                        "SELECT value FROM counters WHERE name = ?1",
                        &[&"counter"],
                        |row| row.get::<i64>(0),
                    )?
                    .into_iter()
                    .next())
            })
        } else {
            self.session.get(&self.address, "counter", &CounterCodec(1))
        };
        result.map_err(|e| e.to_string())
    }
}

// Subscription drains are batches; preserve the tail across bounded kernel polls.
struct StorageProducer {
    subscription: Subscription,
    pending: VecDeque<phenix_storage::Notice>,
}
impl Producer for StorageProducer {
    fn poll(&mut self) -> Result<Option<InputEvent>, phenix_kernel::CallbackError> {
        if self.pending.is_empty() {
            self.pending.extend(self.subscription.drain()?);
        }
        Ok(self.pending.pop_front().map(|notice| InputEvent {
            input: "storage_changed".into(),
            value: Value::local(notice),
        }))
    }
}

pub type Reads = Arc<Mutex<Vec<Result<Option<i64>, String>>>>;
pub type Observed = Arc<Mutex<Vec<phenix_storage::Notice>>>;
struct CounterPlugin {
    counter: Counter,
    reads: Reads,
}
impl PluginInstance for CounterPlugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let counter = self.counter.clone();
        let reads = self.reads.clone();
        registrar.add(Contribution {
            id: "module".into(),
            requires: vec![],
            routes: vec![],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                let mut module = NetworkBuilder::new();
                let writer = counter.clone();
                let write = module.add(
                    "write",
                    support::local_map(move |value: i64| writer.write(value)),
                )?;
                let reader = counter.clone();
                let read = module.add(
                    "read",
                    support::local_map(move |written: Result<(), String>| {
                        written.and_then(|()| reader.read())
                    }),
                )?;
                module.connect(
                    write.output::<Result<(), String>>("output")?,
                    read.input::<Result<(), String>>("input")?,
                    support::capacity(),
                )?;
                module.expose_input("input", write.input::<i64>("input")?, support::capacity())?;
                let log = reads.clone();
                let sink = module.add(
                    "read_result",
                    support::local_sink(move |value: Result<Option<i64>, String>| {
                        log.lock().unwrap().push(value);
                    }),
                )?;
                module.connect(
                    read.output::<Result<Option<i64>, String>>("output")?,
                    sink.input::<Result<Option<i64>, String>>("input")?,
                    support::capacity(),
                )?;
                Ok(CandidateArtifacts {
                    patch: Some(support::input_patch(
                        "counter",
                        "write_counter",
                        Arc::new(ComponentDef::composite(module.build()?)),
                    )),
                    ..Default::default()
                })
            })),
        })
    }
}
struct ObserverPlugin {
    session: Session,
    scope: Scope,
    observed: Observed,
    capacity: usize,
}
impl PluginInstance for ObserverPlugin {
    fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
        let observed = self.observed.clone();
        let session = self.session.clone();
        let scope = self.scope.clone();
        let capacity = self.capacity;
        registrar.add(Contribution {
            id: "observer".into(),
            requires: vec![Key::new("counter", "module")],
            routes: vec!["storage_changed".into()],
            patch: NetworkPatch::new([]),
            prepare: Some(Box::new(move |_| {
                let mut context = CandidateArtifacts::default();
                let log = observed.clone();
                let sink = support::local_sink(move |notice: phenix_storage::Notice| {
                    log.lock().unwrap().push(notice);
                });
                context.patch = Some(support::input_patch("observe", "storage_changed", sink));
                let session = session.clone();
                let scope = scope.clone();
                context.activate(move || {
                    Ok(Box::new(StorageProducer {
                        subscription: session.subscribe(scope.clone(), capacity)?,
                        pending: VecDeque::new(),
                    }))
                })?;
                Ok(context)
            })),
        })
    }
}

pub struct Demo {
    pub kernel: Kernel,
    pub counter: Counter,
    pub reads: Reads,
    pub observed: Observed,
}
pub fn build(sqlite: bool, capacity: usize) -> Result<Demo, String> {
    let registry = Registry::new();
    if sqlite {
        registry.mount_sqlite("main", ":memory:")
    } else {
        registry.mount_memory("main")
    }
    .map_err(|e| e.to_string())?;
    let address = Address::parse("store://main/counters").map_err(|e| e.to_string())?;
    registry
        .register(&address, CounterCodec(1).schema())
        .map_err(|e| e.to_string())?;
    let session = registry.session([Grant::new(
        Scope::Mount("main".into()),
        [Operation::Read, Operation::Write, Operation::Sql],
    )]);
    if sqlite {
        session
            .transaction(&address, |sql| {
                sql.execute(
                    "CREATE TABLE counters (name TEXT PRIMARY KEY, value INTEGER NOT NULL)",
                    &[],
                )?;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
    }
    let counter = Counter {
        session,
        address: address.clone(),
        sqlite,
    };
    let reads = Reads::default();
    let observed = Observed::default();
    let mut catalog = PluginCatalog::new();
    let writer = catalog
        .register("counter", |plugin: CounterPlugin| Ok(plugin))
        .map_err(|e| e.to_string())?;
    let observer = catalog
        .register("observer", |plugin: ObserverPlugin| Ok(plugin))
        .map_err(|e| e.to_string())?;
    let mut plugins = catalog.instance();
    plugins
        .load(
            &observer,
            ObserverPlugin {
                session: registry.session([Grant::new(
                    Scope::Dataset(address.clone()),
                    [Operation::Subscribe],
                )]),
                scope: Scope::Dataset(address),
                observed: observed.clone(),
                capacity,
            },
        )
        .map_err(|e| e.to_string())?;
    plugins
        .load(
            &writer,
            CounterPlugin {
                counter: counter.clone(),
                reads: reads.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
    let mut options = BuildOptions::default();
    support::allow(
        &mut options,
        "counter",
        "module",
        "counter",
        "write_counter",
    );
    support::allow(
        &mut options,
        "observer",
        "observer",
        "observe",
        "storage_changed",
    );
    let kernel = Kernel::build_with_options(
        "storage",
        plugins.snapshots(),
        Arc::new(LocalProcessDispatcher),
        options,
    )
    .map_err(|error| error.to_string())?;
    Ok(Demo {
        kernel,
        counter,
        reads,
        observed,
    })
}

#[allow(dead_code)]
fn main() -> Result<(), String> {
    for sqlite in [false, true] {
        let mut demo = build(sqlite, 8)?;
        demo.kernel.start().map_err(|error| error.to_string())?;
        demo.kernel
            .inject("write_counter", Value::local(42_i64))
            .map_err(|error| error.to_string())?;
        demo.kernel.run_steps(64)?;
        assert_eq!(demo.counter.read()?, Some(42));
        for event in demo.kernel.drain(8).unwrap() {
            event.result.map_err(|error| error.to_string())?;
        }
        demo.kernel.run_steps(64)?;
        assert_eq!(demo.observed.lock().unwrap().len(), 1);
    }
    scoped_storage::run()?;
    Ok(())
}
