#[path = "../examples/scoped_storage.rs"]
mod example;

use example::{
    build,
    observer::ObservedConfig,
    writer::{AgentConfig, Update},
};
use phenix_graph::Value;
use phenix_storage::{
    Error, Revision,
    scoped::{Error as ScopedError, StorageClass},
};

fn configure(demo: &mut example::Demo, expected: Option<Revision>, model: &str) -> Revision {
    assert!(
        demo.kernel
            .fire(
                "configure",
                vec![Value::local(Update {
                    expected,
                    config: AgentConfig {
                        model: model.into()
                    },
                })]
            )
            .unwrap()
            .success()
    );
    demo.writes
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone()
}

#[test]
fn independent_plugins_drive_real_storage_waves_and_enforce_grants() {
    let root = tempfile::tempdir().unwrap();
    let mut demo = build(root.path(), "alpha").unwrap();
    let writer = demo
        .writer
        .project()
        .unwrap()
        .config::<AgentConfig>("phenix.agent/settings")
        .unwrap();
    let reader = demo
        .observer
        .project()
        .unwrap()
        .config::<ObservedConfig>("phenix.agent/settings")
        .unwrap();
    assert!(matches!(writer.get("default"), Err(Error::Denied)));
    assert!(matches!(
        reader.put("default", &ObservedConfig("denied".into())),
        Err(Error::Denied)
    ));
    assert!(matches!(
        reader.compare_exchange("default", None, &ObservedConfig("denied".into())),
        Err(Error::Denied)
    ));
    assert!(matches!(writer.changed(8), Err(Error::Denied)));
    assert!(matches!(
        demo.observer
            .project()
            .unwrap()
            .sql(StorageClass::Data, "phenix.code/index"),
        Err(ScopedError::UnknownResource)
    ));
    let revision = configure(&mut demo, None, "first");
    assert!(demo.reads.lock().unwrap().is_empty());
    let events = demo.kernel.drain(1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].producer.owner, "observer");
    for event in events {
        assert!(event.result.unwrap().success());
    }
    let reads = demo.reads.lock().unwrap();
    let observed = reads[0].as_ref().unwrap().as_ref().unwrap();
    assert_eq!(observed.value.0, "first");
    assert_eq!(observed.revision, revision);
    drop(reads);
    assert!(demo.kernel.drain(8).is_empty());
    let next = configure(&mut demo, Some(revision.clone()), "second");
    assert_ne!(revision, next);
    assert!(
        writer
            .compare_exchange(
                "default",
                Some(&revision),
                &AgentConfig {
                    model: "stale".into()
                }
            )
            .is_err()
    );
    assert_eq!(reader.get("default").unwrap().unwrap().value.0, "second");
    assert!(
        demo.kernel
            .fire("index", vec![Value::local("Agent::run".to_string())])
            .unwrap()
            .success()
    );
    assert_eq!(
        demo.evidence.lock().unwrap()[0].as_ref().unwrap(),
        &vec![("Agent::run".into(), "src/mock.rs:7".into())]
    );
    let sql = demo
        .index
        .project()
        .unwrap()
        .sql(StorageClass::Data, "phenix.code/index")
        .unwrap();
    let indexes = sql
        .transaction(|tx| {
            tx.query(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name = 'symbols_evidence'",
                &[],
                |row| row.get::<String>(0),
            )
        })
        .unwrap();
    assert_eq!(indexes, ["symbols_evidence"]);
}

#[test]
fn restart_preserves_data_and_revisions_while_project_switch_isolates_both_resources() {
    let root = tempfile::tempdir().unwrap();
    let revision = {
        let mut demo = build(root.path(), "alpha").unwrap();
        let revision = configure(&mut demo, None, "persisted");
        assert!(
            demo.kernel
                .fire("index", vec![Value::local("SavedSymbol".to_string())])
                .unwrap()
                .success()
        );
        assert!(demo.evidence.lock().unwrap()[0].is_ok());
        revision
    };
    {
        let mut demo = build(root.path(), "alpha").unwrap();
        let reader = demo
            .observer
            .project()
            .unwrap()
            .config::<ObservedConfig>("phenix.agent/settings")
            .unwrap();
        let found = reader.get("default").unwrap().unwrap();
        assert_eq!(found.revision, revision);
        assert_eq!(found.value.0, "persisted");
        let sql = demo
            .index
            .project()
            .unwrap()
            .sql(StorageClass::Data, "phenix.code/index")
            .unwrap();
        let names = sql
            .transaction(|tx| tx.query("SELECT name FROM symbols", &[], |row| row.get::<String>(0)))
            .unwrap();
        assert_eq!(names, ["SavedSymbol"]);
        configure(&mut demo, Some(revision.clone()), "after-restart");
        let events = demo.kernel.drain(8);
        assert_eq!(events.len(), 1);
        for event in events {
            assert!(event.result.unwrap().success());
        }
    }
    {
        let mut demo = build(root.path(), "beta").unwrap();
        let reader = demo
            .observer
            .project()
            .unwrap()
            .config::<ObservedConfig>("phenix.agent/settings")
            .unwrap();
        assert!(reader.get("default").unwrap().is_none());
        configure(&mut demo, None, "isolated");
        assert!(
            demo.kernel
                .fire("index", vec![Value::local("OtherSymbol".to_string())])
                .unwrap()
                .success()
        );
        assert_eq!(
            demo.evidence.lock().unwrap()[0].as_ref().unwrap(),
            &vec![("OtherSymbol".into(), "src/mock.rs:7".into())]
        );
    }
    let demo = build(root.path(), "alpha").unwrap();
    let found = demo
        .observer
        .project()
        .unwrap()
        .config::<ObservedConfig>("phenix.agent/settings")
        .unwrap()
        .get("default")
        .unwrap()
        .unwrap();
    assert_eq!(found.value.0, "after-restart");
    assert_ne!(found.revision, revision);
}

#[test]
fn public_entry_point_runs() {
    example::run().unwrap();
}

#[test]
fn fixed_host_policy_rejects_extra_requests_before_storage_injection() {
    use phenix_graph::LocalExecutor;
    use phenix_kernel::{Contribution, Kernel};
    use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
    use phenix_storage::{
        Operation,
        scoped::{Identity, ScopeKind},
    };
    use phenix_storage_plugin::{PreparedStorage, ResourceOffer, ResourceRequirements};
    use std::sync::Arc;

    struct RogueWriter(Operation);
    impl PluginInstance for RogueWriter {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            example::writer::Plugin {
                storage: Default::default(),
                writes: Default::default(),
            }
            .contribute(registrar)?;
            registrar.add(ResourceOffer::kv::<AgentConfig>(
                "extra",
                ScopeKind::Project,
                StorageClass::Config,
                "phenix.agent/settings",
                [self.0],
            ))
        }
    }

    // Read is allowed on the resource but not for the writer; Delete is allowed on neither.
    for extra in [Operation::Read, Operation::Delete] {
        let root = tempfile::tempdir().unwrap();
        let storage_root = root.path().join("unopened");
        let identity = Identity::user("demo").unwrap().project("alpha").unwrap();
        let mut catalog = PluginCatalog::new();
        let writer = catalog
            .register("writer", |op| Ok(RogueWriter(op)))
            .unwrap();
        let observer = catalog
            .register("observer", |p: example::observer::Plugin| Ok(p))
            .unwrap();
        let index = catalog
            .register("code-index", |p: example::code_index::Plugin| Ok(p))
            .unwrap();
        let mut plugins = catalog.instance();
        plugins.load(&writer, extra).unwrap();
        plugins
            .load(
                &observer,
                example::observer::Plugin {
                    storage: Default::default(),
                    reads: Default::default(),
                },
            )
            .unwrap();
        plugins
            .load(
                &index,
                example::code_index::Plugin {
                    storage: Default::default(),
                    results: Default::default(),
                },
            )
            .unwrap();
        let snapshots: Vec<_> = plugins.snapshots().collect();
        for (owner, id) in [
            ("writer", "writer"),
            ("observer", "observer"),
            ("code-index", "index"),
        ] {
            assert!(
                snapshots
                    .iter()
                    .find(|s| s.name() == owner)
                    .unwrap()
                    .get::<Contribution>(id)
                    .is_some()
            );
        }
        // Graphs are already present, but cannot compose until the host injects storage.
        let error = Kernel::build("not-injected", snapshots.clone(), Arc::new(LocalExecutor))
            .err()
            .expect("composition must require storage injection");
        assert!(error.contains("storage not injected"), "{error}");
        let requirements = ResourceRequirements::collect(snapshots, &identity).unwrap();
        assert_eq!(requirements.iter().count(), 2);
        let policy = example::host_policy(&storage_root, &identity).unwrap();
        assert!(matches!(
            PreparedStorage::prepare(requirements, identity, policy),
            Err(phenix_storage_plugin::Error::Denied)
        ));
        assert!(!storage_root.exists());
    }
}
