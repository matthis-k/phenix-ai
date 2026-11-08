use phenix_graph::{GraphBuilder, LocalExecutor, Value};
use phenix_kernel::{Contribution, Kernel, Key, Notice, Producer};
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
    fn poll(&mut self) -> Result<Option<Notice>, String> {
        if self.pending.is_empty() {
            self.pending
                .extend(self.subscription.drain().map_err(|e| e.to_string())?);
        }
        Ok(self.pending.pop_front().map(|notice| Notice {
            trigger: "storage_changed".into(),
            values: vec![Value::local(notice)],
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
            compose: Box::new(move |graph| {
                let mut module = GraphBuilder::new("counter-module");
                let writer = counter.clone();
                let write = module
                    .node("write", move |value: i64| writer.write(value))
                    .map_err(|e| e.to_string())?;
                let reader = counter.clone();
                let read = module
                    .node("read", move |written: Result<(), String>| {
                        written.and_then(|()| reader.read())
                    })
                    .map_err(|e| e.to_string())?;
                module.connect(write.output(), read.inlet())?;
                module.expose_input("value", write.inlet())?;
                module.expose_output("value", read.output())?;
                let instance = graph.instance("counter", &module.module())?;
                let log = reads.clone();
                let sink = graph
                    .node("read_result", move |value: Result<Option<i64>, String>| {
                        log.lock().unwrap().push(value);
                    })
                    .map_err(|e| e.to_string())?;
                graph.connect(
                    instance.local_output::<Result<Option<i64>, String>>("value")?,
                    sink.inlet(),
                )?;
                let input =
                    graph.expose_input("counter_value", instance.local_input::<i64>("value")?)?;
                graph.trigger("write_counter", [input])?;
                Ok(())
            }),
            activate: None,
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
            compose: Box::new(move |graph| {
                let log = observed.clone();
                let sink = graph
                    .node("observe", move |notice: phenix_storage::Notice| {
                        log.lock().unwrap().push(notice);
                    })
                    .map_err(|e| e.to_string())?;
                let input = graph.expose_input("storage_notice", sink.inlet())?;
                graph.trigger("storage_changed", [input])?;
                Ok(())
            }),
            activate: Some(Box::new(move || {
                Ok(Box::new(StorageProducer {
                    subscription: session
                        .subscribe(scope.clone(), capacity)
                        .map_err(|e| e.to_string())?,
                    pending: VecDeque::new(),
                }))
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
    let kernel = Kernel::build("storage", plugins.snapshots(), Arc::new(LocalExecutor))?;
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
        demo.kernel.start()?;
        let write = demo
            .kernel
            .fire("write_counter", vec![Value::local(42_i64)])?;
        assert!(write.success());
        assert_eq!(demo.counter.read()?, Some(42));
        for event in demo.kernel.drain(8) {
            assert!(
                event
                    .result
                    .map_err(|error| format!("{error:?}"))?
                    .success()
            );
        }
        assert_eq!(demo.observed.lock().unwrap().len(), 1);
    }
    Ok(())
}
