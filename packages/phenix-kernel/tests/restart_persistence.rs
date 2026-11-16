mod support;

use phenix_graph::{
    Interface, PortDef, Value,
    component::ComponentDef,
    compose::{Edit, Footprint, NetworkPatch},
    process::{HandoffError, LocalProcessDispatcher, Process, ProcessContext, Step},
};
use phenix_kernel::{
    BuildOptions, CandidateArtifacts, Contribution, Key,
    host::{Candidate, Generation, KernelHost, LifecycleError},
};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar, StagedPlugin};
use phenix_storage::{
    Address, Codec, Error, Grant, Operation, Registry, Revision, Revisioned, Schema, Scope,
    scoped::{
        CellSpec, Error as ScopedError, Identity, Kv as StorageKv, ReadCell, ResourceCatalog,
        ResourceId, ScopeKind, Storage, StorageClass, Stored,
    },
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

const CELL: CellSpec<Counter> = CellSpec::project_state("phenix.c6/counter");

#[derive(Debug, PartialEq, Eq)]
struct Counter(i64);

#[derive(Default)]
struct Format;
impl Codec for Format {
    type Value = Counter;

    fn schema(&self) -> Schema {
        Schema {
            name: "phenix.c6.counter".into(),
            revision: 1,
            structure: "counter:i64-le".into(),
            codec: "i64-le".into(),
        }
    }

    fn encode(&self, value: &Counter) -> phenix_storage::Result<Vec<u8>> {
        Ok(value.0.to_le_bytes().to_vec())
    }

    fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<Counter> {
        let bytes = bytes
            .try_into()
            .map_err(|_| Error::Codec("expected i64".into()))?;
        Ok(Counter(i64::from_le_bytes(bytes)))
    }
}
impl Stored for Counter {
    type Format = Format;
}

struct Views {
    counter: Arc<Storage>,
    reader: Storage,
    writer: Storage,
}

// Trusted-host placement and grants, never derived from a plugin request.
// Every call opens a new registry/backend, including while old leases survive.
fn reopen(path: &Path, schema: Schema) -> phenix_storage::Result<Views> {
    let registry = Registry::new();
    registry.mount_sqlite("c6", path)?;
    let address = Address::parse("store://c6/counter")?;
    registry.register(&address, schema)?;
    let unrelated = Address::parse("store://c6/unrelated")?;
    registry.register(&unrelated, Format.schema())?;
    let identity = Identity::user("c6")
        .unwrap()
        .project("project-one")
        .unwrap();
    let scope = identity.resolve(ScopeKind::Project).unwrap();
    let mut catalog = ResourceCatalog::new();
    catalog
        .bind(
            CELL.resource_spec().resolve(&identity).unwrap(),
            address.clone(),
        )
        .unwrap();
    catalog
        .bind(
            ResourceId::new(scope, StorageClass::State, "phenix.c6/unrelated").unwrap(),
            unrelated,
        )
        .unwrap();
    let bindings = catalog.freeze();
    let view = |operations: Vec<Operation>| {
        bindings.access(
            registry.session([Grant::new(Scope::Dataset(address.clone()), operations)]),
            identity.clone(),
        )
    };
    Ok(Views {
        counter: Arc::new(view(vec![Operation::Read, Operation::Write])),
        reader: view(vec![Operation::Read]),
        writer: view(vec![Operation::Write]),
    })
}

fn kv(storage: &Storage) -> StorageKv<Counter> {
    storage
        .project()
        .unwrap()
        .state("phenix.c6/counter")
        .unwrap()
}

fn assert_grants(views: &Views) {
    assert!(matches!(kv(&views.writer).get("value"), Err(Error::Denied)));
    assert!(matches!(
        kv(&views.writer).get_revisioned("value"),
        Err(Error::Denied)
    ));
    assert!(matches!(
        views.writer.bind(CELL.read()),
        Err(ScopedError::Storage(Error::Denied))
    ));
    assert!(matches!(
        kv(&views.reader).put("value", &Counter(-1)),
        Err(Error::Denied)
    ));
    assert!(matches!(
        kv(&views.reader).compare_exchange("value", None, &Counter(-1)),
        Err(Error::Denied)
    ));
    for storage in [&*views.counter, &views.reader, &views.writer] {
        let unrelated = storage
            .project()
            .unwrap()
            .state::<Counter>("phenix.c6/unrelated")
            .unwrap();
        assert!(matches!(unrelated.get("value"), Err(Error::Denied)));
        assert!(matches!(
            unrelated.put("value", &Counter(-1)),
            Err(Error::Denied)
        ));
        assert!(matches!(kv(storage).delete("value"), Err(Error::Denied)));
        assert!(matches!(kv(storage).changed(1), Err(Error::Denied)));
        assert!(matches!(
            storage.user().state::<Counter>("phenix.c6/counter"),
            Err(ScopedError::UnknownResource)
        ));
        assert!(matches!(
            storage
                .project()
                .unwrap()
                .sql(StorageClass::State, "phenix.c6/counter"),
            Err(ScopedError::WrongResourceKind)
        ));
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Write {
        version: u64,
        private: i64,
        before: i64,
        after: i64,
    },
    ProcessDrop {
        version: u64,
        private: i64,
    },
    PluginDrop(u64),
}
type Log = Arc<Mutex<Vec<Event>>>;

struct CountingProcess {
    version: u64,
    private: i64,
    cell: ReadCell<Counter>,
    kv: Arc<StorageKv<Counter>>,
    log: Log,
}
impl Process for CountingProcess {
    fn step(&mut self, context: &mut ProcessContext) -> Result<Step, String> {
        let Some(value) = context.take("run") else {
            return Ok(Step::Finished);
        };
        self.private += value.borrow::<i64>().ok_or("expected i64")?;
        let found = self.cell.optional().get().map_err(|e| e.to_string())?;
        let before = found.as_ref().map_or(0, |v| v.value.0);
        let after = before + self.private;
        self.kv
            .compare_exchange(
                "value",
                found.as_ref().map(|v| &v.revision),
                &Counter(after),
            )
            .map_err(|e| e.to_string())?;
        self.log.lock().unwrap().push(Event::Write {
            version: self.version,
            private: self.private,
            before,
            after,
        });
        Ok(Step::Complete)
    }
}
impl Drop for CountingProcess {
    fn drop(&mut self) {
        self.log.lock().unwrap().push(Event::ProcessDrop {
            version: self.version,
            private: self.private,
        });
    }
}

struct Plugin {
    version: u64,
    storage: Arc<Storage>,
    log: Log,
}
impl Drop for Plugin {
    fn drop(&mut self) {
        self.log
            .lock()
            .unwrap()
            .push(Event::PluginDrop(self.version));
    }
}
impl PluginInstance for Plugin {
    fn contribute(&self, into: &mut Registrar) -> Result<(), Failure> {
        let version = self.version;
        let storage = self.storage.clone();
        let log = self.log.clone();
        Contribution::new("counter", NetworkPatch::new([]))
            .prepare(move |_| {
                let cell = storage.bind(CELL.read())?;
                let kv = Arc::new(kv(&storage));
                let log = log.clone();
                let definition = Arc::new(ComponentDef::primitive(
                    Arc::new(Interface::new([PortDef::local::<i64>("run")], []).unwrap()),
                    Arc::new(move || CountingProcess {
                        version,
                        private: 0,
                        cell: cell.clone(),
                        kv: kv.clone(),
                        log: log.clone(),
                    }),
                ));
                Ok(CandidateArtifacts {
                    patch: Some(NetworkPatch::new([
                        Edit::Add {
                            path: "counter".into(),
                            definition,
                        },
                        Edit::ExposeInput {
                            name: "increment".into(),
                            targets: vec![support::path("counter", "run")],
                            capacity: support::capacity(),
                        },
                    ])),
                    ..Default::default()
                })
            })
            .register(into)
    }
}

fn prepare(
    host: &KernelHost,
    stage: StagedPlugin,
    bind: impl FnOnce(&phenix_kernel::host::Binding<'_>) -> Result<(), phenix_kernel::CallbackError>,
) -> Result<Candidate, LifecycleError> {
    let mut options = BuildOptions::default();
    support::grant(
        &mut options,
        Key::new("counter", "counter"),
        [
            Footprint::Component("counter".into()),
            Footprint::InputExport("increment".into()),
        ],
    );
    host.prepare(
        vec![stage],
        options,
        Arc::new(LocalProcessDispatcher),
        "c6",
        bind,
    )
}

fn fire(generation: &Generation) {
    generation.inject("increment", Value::local(1_i64)).unwrap();
    generation.run_steps(32).unwrap();
    generation
        .with_session(|s| assert_eq!(s.failure(), None))
        .unwrap();
}

fn read(views: &Views) -> Revisioned<Counter> {
    let cell = views.reader.bind(CELL.read()).unwrap().get().unwrap();
    assert_eq!(
        kv(&views.reader).get_revisioned("value").unwrap().unwrap(),
        cell
    );
    cell
}

#[test]
fn public_host_restarts_private_state_but_persists_granted_versioned_cells_and_archival_jobs() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("c6.sqlite");
    let log = Log::default();
    let final_revision: Revision;
    {
        let mut catalog = PluginCatalog::new();
        let key = catalog.register("counter", |p: Plugin| Ok(p)).unwrap();
        let mut host = KernelHost::new(catalog.instance());
        let views = reopen(&path, Format.schema()).unwrap();
        assert_grants(&views);
        let initial = prepare(
            &host,
            host.plugins()
                .prepare_load(
                    &key,
                    Plugin {
                        version: 1,
                        storage: views.counter.clone(),
                        log: log.clone(),
                    },
                )
                .unwrap(),
            |_| Ok(()),
        )
        .unwrap();
        assert!(host.active().is_none());
        assert!(log.lock().unwrap().is_empty());
        assert!(kv(&views.reader).get_revisioned("value").unwrap().is_none());
        let old = host.publish(initial).unwrap();
        assert_eq!(old.epoch(), 1);
        fire(&old);
        fire(&old);
        let before = read(&views);
        assert_eq!(before.value, Counter(3));

        // Same file, genuinely new registry/backend, not just the same Storage Arc.
        let successor_views = reopen(&path, Format.schema()).unwrap();
        assert!(!Arc::ptr_eq(&views.counter, &successor_views.counter));
        assert_eq!(read(&successor_views), before);
        let replacement = prepare(
            &host,
            host.plugins()
                .prepare_reload(
                    &key,
                    Plugin {
                        version: 2,
                        storage: successor_views.counter.clone(),
                        log: log.clone(),
                    },
                )
                .unwrap(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);
        assert_eq!(
            read(&successor_views),
            before,
            "preparation did not run the process"
        );

        old.inject("increment", Value::local(1_i64)).unwrap();
        let (_, retained_job) = old.with_session(|s| s.next_job().unwrap()).unwrap();
        let current = host.publish(replacement).unwrap();
        assert_eq!(current.epoch(), 2);
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 2);
        fire(&current); // Fresh private counter: 1; durable continuity: 3 -> 4.
        assert_eq!(read(&successor_views).value, Counter(4));

        // The retained archival writer is allowed, not fenced by publication.
        let completion = retained_job.run();
        let (error, completion) = current
            .with_session(|s| s.complete_job(completion))
            .unwrap()
            .unwrap_err();
        assert_eq!(error, HandoffError::ForeignCompletion);
        old.with_session(|s| s.complete_job(*completion).unwrap())
            .unwrap();
        old.run_steps(32).unwrap();
        assert_eq!(read(&successor_views).value, Counter(7));
        fire(&old); // Explicit archival execution remains usable: private 4.
        assert_eq!(read(&successor_views).value, Counter(11));
        fire(&current); // Successor's own private state is now 2, not old's 5.
        let latest = read(&successor_views);
        assert_eq!(latest.value, Counter(13));
        assert!(matches!(
            kv(&views.writer).compare_exchange("value", Some(&before.revision), &Counter(-1)),
            Err(Error::Conflict { current: Some(r) }) if r == latest.revision
        ));
        assert_eq!(read(&views), latest);
        assert_grants(&views);
        assert_grants(&successor_views);
        assert_eq!(
            &*log.lock().unwrap(),
            &[
                Event::Write {
                    version: 1,
                    private: 1,
                    before: 0,
                    after: 1
                },
                Event::Write {
                    version: 1,
                    private: 2,
                    before: 1,
                    after: 3
                },
                Event::Write {
                    version: 2,
                    private: 1,
                    before: 3,
                    after: 4
                },
                Event::Write {
                    version: 1,
                    private: 3,
                    before: 4,
                    after: 7
                },
                Event::Write {
                    version: 1,
                    private: 4,
                    before: 7,
                    after: 11
                },
                Event::Write {
                    version: 2,
                    private: 2,
                    before: 11,
                    after: 13
                },
            ]
        );

        // An owned job alone also pins old private state and plugin code.
        old.inject("increment", Value::local(1_i64)).unwrap();
        let (_, last_job) = old.with_session(|s| s.next_job().unwrap()).unwrap();
        drop(old);
        assert!(!log.lock().unwrap().contains(&Event::PluginDrop(1)));
        let completion = last_job.run();
        assert_eq!(read(&successor_views).value, Counter(18));
        assert!(!log.lock().unwrap().contains(&Event::PluginDrop(1)));
        drop(completion);
        let events = log.lock().unwrap();
        let process = events
            .iter()
            .position(|e| {
                *e == Event::ProcessDrop {
                    version: 1,
                    private: 5,
                }
            })
            .unwrap();
        let plugin = events
            .iter()
            .position(|e| *e == Event::PluginDrop(1))
            .unwrap();
        assert!(process < plugin);
        drop(events);
        final_revision = read(&successor_views).revision;
    }
    assert!(log.lock().unwrap().contains(&Event::ProcessDrop {
        version: 2,
        private: 2
    }));
    assert!(log.lock().unwrap().contains(&Event::PluginDrop(2)));

    // All hosts, generations, jobs, views and registries above have been dropped.
    let views = reopen(&path, Format.schema()).unwrap();
    assert_eq!(
        read(&views),
        Revisioned {
            value: Counter(18),
            revision: final_revision.clone()
        }
    );
    assert_grants(&views);
    assert!(matches!(
        kv(&views.writer).compare_exchange("value", None, &Counter(-1)),
        Err(Error::Conflict { current: Some(r) }) if r == final_revision
    ));
    let next = kv(&views.writer)
        .compare_exchange("value", Some(&final_revision), &Counter(19))
        .unwrap();
    assert_ne!(next, final_revision);
    assert_eq!(
        read(&views),
        Revisioned {
            value: Counter(19),
            revision: next
        }
    );
}

#[test]
fn rejected_schema_and_candidate_keep_active_identity_but_external_effects_are_not_rollback() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("c6.sqlite");
    let log = Log::default();
    let mut catalog = PluginCatalog::new();
    let key = catalog.register("counter", |p: Plugin| Ok(p)).unwrap();
    let mut host = KernelHost::new(catalog.instance());
    let views = reopen(&path, Format.schema()).unwrap();
    let initial = prepare(
        &host,
        host.plugins()
            .prepare_load(
                &key,
                Plugin {
                    version: 1,
                    storage: views.counter.clone(),
                    log: log.clone(),
                },
            )
            .unwrap(),
        |_| Ok(()),
    )
    .unwrap();
    let old = host.publish(initial).unwrap();
    fire(&old);
    let before = read(&views);
    old.inject("increment", Value::local(1_i64)).unwrap();
    let (_, retained_job) = old.with_session(|s| s.next_job().unwrap()).unwrap();

    // Each canonical schema field matters on a real SQLite registry reopen.
    for field in 0..4 {
        let mut schema = Format.schema();
        match field {
            0 => schema.name.push_str(".incompatible"),
            1 => schema.revision += 1,
            2 => schema.structure.push_str(";extra:i64"),
            _ => schema.codec.push_str("-incompatible"),
        }
        assert!(matches!(reopen(&path, schema), Err(Error::SchemaMismatch)));
        assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
        assert_eq!(old.epoch(), 1);
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);
        assert_eq!(read(&views), before);
    }

    let new_views = reopen(&path, Format.schema()).unwrap();
    for external_effect in [false, true] {
        let stage = host
            .plugins()
            .prepare_reload(
                &key,
                Plugin {
                    version: 2,
                    storage: new_views.counter.clone(),
                    log: log.clone(),
                },
            )
            .unwrap();
        let result = prepare(&host, stage, |_| {
            if external_effect {
                // An actual external operation during host preparation, not a process step.
                kv(&new_views.counter).put("external-effect", &Counter(99))?;
            }
            Err("application rejects candidate deterministically".into())
        });
        assert!(matches!(result, Err(LifecycleError::Binding(_))));
        assert!(Arc::ptr_eq(&old, &host.active().unwrap()));
        assert_eq!(old.epoch(), 1);
        assert_eq!(host.plugins().snapshot(&key).unwrap().revision(), 1);
        assert_eq!(read(&views), before);
        assert_eq!(
            kv(&views.reader).get("external-effect").unwrap(),
            external_effect.then_some(Counter(99))
        );
    }
    assert!(
        !log.lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Write { version: 2, .. }))
    );
    let reopened = reopen(&path, Format.schema()).unwrap();
    assert_eq!(
        kv(&reopened.reader).get("external-effect").unwrap(),
        Some(Counter(99))
    );
    assert_eq!(read(&reopened), before);
    old.with_session(|s| s.complete_job(retained_job.run()).unwrap())
        .unwrap();
    old.run_steps(32).unwrap();
    fire(&old);
    assert_eq!(read(&reopened).value, Counter(6));
    assert_grants(&reopened);
}
