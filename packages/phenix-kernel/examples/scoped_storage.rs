//! Three independent plugins; only the host chooses paths and grants.
use phenix_graph::{LocalExecutor, Value};
use phenix_kernel::{Contribution, Kernel};
use phenix_plugin::{Failure, PluginCatalog, PluginInstance, Registrar};
use phenix_storage::scoped::{
    BindingTarget, Identity, ResourceId, ScopeKind, Storage, StorageClass, Stored, XdgPaths,
};
use phenix_storage::{Codec, Operation, Revision, Revisioned, Schema};
use phenix_storage_plugin::{
    BackendPlacement, PreparedStorage, ResourceBinding, ResourceOffer, ResourceRequirements,
    StoragePolicy,
};
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
type Log<T> = Arc<Mutex<Vec<Result<T, String>>>>;
pub type StorageSlot = Arc<OnceLock<Arc<Storage>>>;

pub mod writer {
    use super::*;
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentConfig {
        pub model: String,
    }
    #[derive(Default)]
    pub struct Format;
    impl Codec for Format {
        type Value = AgentConfig;
        fn schema(&self) -> Schema {
            Schema {
                name: "phenix.agent.config".into(),
                revision: 1,
                structure: "model:utf8".into(),
                codec: "utf8".into(),
            }
        }
        fn encode(&self, value: &AgentConfig) -> phenix_storage::Result<Vec<u8>> {
            Ok(value.model.as_bytes().to_vec())
        }
        fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<AgentConfig> {
            String::from_utf8(bytes.to_vec())
                .map(|model| AgentConfig { model })
                .map_err(|e| phenix_storage::Error::Codec(e.to_string()))
        }
    }
    impl Stored for AgentConfig {
        type Format = Format;
    }
    #[derive(Clone)]
    pub struct Update {
        pub expected: Option<Revision>,
        pub config: AgentConfig,
    }
    pub struct Plugin {
        pub storage: StorageSlot,
        pub writes: Log<Revision>,
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            let mut offer = ResourceOffer::kv::<AgentConfig>(
                "config",
                ScopeKind::Project,
                StorageClass::Config,
                "phenix.agent/settings",
                [Operation::Write],
            );
            offer.request = offer.request.with_atomic_cas();
            registrar.add(offer)?;
            let storage = self.storage.clone();
            let writes = self.writes.clone();
            registrar.add(Contribution {
                id: "writer".into(),
                requires: vec![],
                routes: vec![],
                compose: Box::new(move |context| {
                    let graph = &mut *context.graph;
                    let kv = storage
                        .get()
                        .ok_or("writer storage not injected")?
                        .project()
                        .map_err(text)?
                        .config::<AgentConfig>("phenix.agent/settings")
                        .map_err(text)?;
                    let log = writes.clone();
                    let node = graph
                        .node("save_config", move |update: Update| {
                            log.lock().unwrap().push(
                                kv.compare_exchange(
                                    "default",
                                    update.expected.as_ref(),
                                    &update.config,
                                )
                                .map_err(text),
                            );
                        })
                        .map_err(text)?;
                    let input = graph.expose_input("config_update", node.inlet())?;
                    graph.trigger("configure", [input])?;
                    Ok(())
                }),
            })
        }
    }
}

pub mod observer {
    use super::*;
    use phenix_storage_graph::StorageBinding;
    // This plugin owns its type AND wire descriptor; it never imports the writer.
    #[derive(Debug, PartialEq, Eq)]
    pub struct ObservedConfig(pub String);
    #[derive(Default)]
    pub struct Format;
    impl Codec for Format {
        type Value = ObservedConfig;
        fn schema(&self) -> Schema {
            Schema {
                name: "phenix.agent.config".into(),
                revision: 1,
                structure: "model:utf8".into(),
                codec: "utf8".into(),
            }
        }
        fn encode(&self, value: &ObservedConfig) -> phenix_storage::Result<Vec<u8>> {
            Ok(value.0.as_bytes().to_vec())
        }
        fn decode(&self, bytes: &[u8]) -> phenix_storage::Result<ObservedConfig> {
            String::from_utf8(bytes.to_vec())
                .map(ObservedConfig)
                .map_err(|e| phenix_storage::Error::Codec(e.to_string()))
        }
    }
    impl Stored for ObservedConfig {
        type Format = Format;
    }
    pub struct Plugin {
        pub storage: StorageSlot,
        pub reads: Log<Option<Revisioned<ObservedConfig>>>,
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            registrar.add(ResourceOffer::kv_schema(
                "config",
                ScopeKind::Project,
                StorageClass::Config,
                "phenix.agent/settings",
                Format.schema(),
                [Operation::Read, Operation::Subscribe],
            ))?;
            let storage = self.storage.clone();
            let source = storage.clone();
            let reads = self.reads.clone();
            registrar.add(Contribution {
                id: "observer".into(),
                requires: vec![],
                routes: vec!["config_changed".into()],
                compose: Box::new(move |context| {
                    let graph = &mut *context.graph;
                    let kv = storage
                        .get()
                        .ok_or("observer storage not injected")?
                        .project()
                        .map_err(text)?
                        .config::<ObservedConfig>("phenix.agent/settings")
                        .map_err(text)?;
                    let log = reads.clone();
                    let node = graph
                        .node("refresh_config", move |_notice: phenix_storage::Notice| {
                            log.lock().unwrap().push(kv.get("default").map_err(text));
                        })
                        .map_err(text)?;
                    let input = graph.expose_input("config_notice", node.inlet())?;
                    graph.trigger("config_changed", [input])?;
                    let source = source.clone();
                    context.activate(move || {
                        let subscription = source
                            .get()
                            .ok_or("observer storage not injected")?
                            .project()
                            .map_err(text)?
                            .config::<ObservedConfig>("phenix.agent/settings")
                            .map_err(text)?
                            .changed(16)
                            .map_err(text)?;
                        Ok(Box::new(StorageBinding::new(
                            subscription,
                            "config_changed",
                        )))
                    })
                }),
            })
        }
    }
}

pub mod code_index {
    use super::*;
    pub type Evidence = Vec<(String, String)>;
    pub struct Plugin {
        pub storage: StorageSlot,
        pub results: Log<Evidence>,
    }
    impl PluginInstance for Plugin {
        fn contribute(&self, registrar: &mut Registrar) -> Result<(), Failure> {
            registrar.add(ResourceOffer::sql(
                "symbols",
                ScopeKind::Project,
                StorageClass::Data,
                "phenix.code/index",
                [Operation::SqlRead, Operation::SqlWrite, Operation::SqlAdmin],
            ))?;
            let storage = self.storage.clone();
            let results = self.results.clone();
            registrar.add(Contribution {
                id: "index".into(), requires: vec![], routes: vec![],
                compose: Box::new(move |context| {
                    let graph = &mut *context.graph;
                    let sql = storage.get().ok_or("index storage not injected")?.project().map_err(text)?
                        .sql(StorageClass::Data, "phenix.code/index").map_err(text)?;
                    let log = results.clone();
                    let node = graph.node("index_symbols", move |symbol: String| {
                        let result = sql.transaction(|tx| {
                            tx.execute("CREATE TABLE IF NOT EXISTS symbols (name TEXT PRIMARY KEY, evidence TEXT NOT NULL)", &[])?;
                            tx.execute("CREATE INDEX IF NOT EXISTS symbols_evidence ON symbols(evidence)", &[])?;
                            tx.execute("INSERT INTO symbols(name, evidence) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET evidence = excluded.evidence",
                                &[&symbol, &"src/mock.rs:7"])?;
                            tx.query("SELECT name, evidence FROM symbols WHERE evidence = ?1 ORDER BY name",
                                &[&"src/mock.rs:7"], |row| Ok((row.get::<String>(0)?, row.get::<String>(1)?)))
                        }).map_err(text);
                        log.lock().unwrap().push(result);
                    }).map_err(text)?;
                    let input = graph.expose_input("symbol", node.inlet())?;
                    graph.trigger("index", [input])?;
                    Ok(())
                }),
            })
        }
    }
}

pub struct Demo {
    pub kernel: Kernel,
    pub writer: Arc<Storage>,
    pub observer: Arc<Storage>,
    pub index: Arc<Storage>,
    pub writes: Log<Revision>,
    pub reads: Log<Option<Revisioned<observer::ObservedConfig>>>,
    pub evidence: Log<code_index::Evidence>,
}

// Host allowlists are fixed decisions, never derived from plugin requests.
pub fn host_policy(root: &Path, identity: &Identity) -> Result<StoragePolicy, String> {
    let scope = identity.resolve(ScopeKind::Project).map_err(text)?;
    let config = ResourceId::new(scope.clone(), StorageClass::Config, "phenix.agent/settings")
        .map_err(text)?;
    let index = ResourceId::new(scope, StorageClass::Data, "phenix.code/index").map_err(text)?;
    let paths = XdgPaths::new(
        root.join("config"),
        root.join("data"),
        root.join("state"),
        root.join("cache"),
        None,
    )
    .map_err(text)?;
    let path = paths
        .directory(&config)
        .map_err(text)?
        .join("project.sqlite");
    let mut policy = StoragePolicy::default();
    policy
        .mounts
        .insert("project".into(), BackendPlacement::Sqlite(path));
    // SQL shares this mount's application tables; reserved storage tables remain blocked.
    policy.shared_sql_mounts.insert("project".into());
    policy.resources.insert(
        config.clone(),
        ResourceBinding {
            target: BindingTarget::Dataset(
                phenix_storage::Address::parse("store://project/settings").map_err(text)?,
            ),
            allowed: [Operation::Write, Operation::Read, Operation::Subscribe].into(),
        },
    );
    policy.resources.insert(
        index.clone(),
        ResourceBinding {
            target: BindingTarget::SqlMount("project".into()),
            allowed: [Operation::SqlRead, Operation::SqlWrite, Operation::SqlAdmin].into(),
        },
    );
    policy.plugin_access.insert(
        "writer".into(),
        [(config.clone(), [Operation::Write].into())].into(),
    );
    policy.plugin_access.insert(
        "observer".into(),
        [(config, [Operation::Read, Operation::Subscribe].into())].into(),
    );
    policy.plugin_access.insert(
        "code-index".into(),
        [(
            index,
            [Operation::SqlRead, Operation::SqlWrite, Operation::SqlAdmin].into(),
        )]
        .into(),
    );
    Ok(policy)
}

pub fn build(root: &Path, project: &str) -> Result<Demo, String> {
    let writes = Log::default();
    let reads = Log::default();
    let evidence = Log::default();
    let mut catalog = PluginCatalog::new();
    let writer = catalog
        .register("writer", |p: writer::Plugin| Ok(p))
        .map_err(text)?;
    let observer = catalog
        .register("observer", |p: observer::Plugin| Ok(p))
        .map_err(text)?;
    let index = catalog
        .register("code-index", |p: code_index::Plugin| Ok(p))
        .map_err(text)?;
    let writer_slot = StorageSlot::default();
    let observer_slot = StorageSlot::default();
    let index_slot = StorageSlot::default();
    let mut plugins = catalog.instance();
    plugins
        .load(
            &writer,
            writer::Plugin {
                storage: writer_slot.clone(),
                writes: writes.clone(),
            },
        )
        .map_err(text)?;
    plugins
        .load(
            &observer,
            observer::Plugin {
                storage: observer_slot.clone(),
                reads: reads.clone(),
            },
        )
        .map_err(text)?;
    plugins
        .load(
            &index,
            code_index::Plugin {
                storage: index_slot.clone(),
                results: evidence.clone(),
            },
        )
        .map_err(text)?;
    let identity = Identity::user("demo")
        .map_err(text)?
        .project(project)
        .map_err(text)?;
    // Plan and compose the exact same snapshots, including their deferred graphs.
    let snapshots: Vec<_> = plugins.snapshots().collect();
    let requirements = ResourceRequirements::collect(snapshots.clone(), &identity).map_err(text)?;
    let policy = host_policy(root, &identity)?;
    let mut active = PreparedStorage::prepare(requirements, identity, policy)
        .map_err(text)?
        .activate()
        .map_err(text)?;
    let writer_view = Arc::new(active.views.remove("writer").ok_or("writer view")?);
    let observer_view = Arc::new(active.views.remove("observer").ok_or("observer view")?);
    let index_view = Arc::new(active.views.remove("code-index").ok_or("index view")?);
    writer_slot
        .set(writer_view.clone())
        .map_err(|_| "writer already injected")?;
    observer_slot
        .set(observer_view.clone())
        .map_err(|_| "observer already injected")?;
    index_slot
        .set(index_view.clone())
        .map_err(|_| "index already injected")?;
    let mut kernel = Kernel::build("scoped-storage", snapshots, Arc::new(LocalExecutor))?;
    kernel.start()?;
    Ok(Demo {
        kernel,
        writer: writer_view,
        observer: observer_view,
        index: index_view,
        writes,
        reads,
        evidence,
    })
}

pub fn run() -> Result<(), String> {
    let root = tempfile::tempdir().map_err(text)?;
    let mut demo = build(root.path(), "project-one")?;
    assert!(
        demo.kernel
            .fire(
                "configure",
                vec![Value::local(writer::Update {
                    expected: None,
                    config: writer::AgentConfig {
                        model: "local-model".into()
                    },
                })]
            )?
            .success()
    );
    assert!(demo.writes.lock().unwrap()[0].is_ok());
    let events = demo.kernel.drain(16);
    assert_eq!(events.len(), 1);
    for event in events {
        assert!(event.result.map_err(|e| format!("{e:?}"))?.success());
    }
    assert_eq!(
        demo.reads.lock().unwrap()[0]
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .value
            .0,
        "local-model"
    );
    assert!(
        demo.kernel
            .fire("index", vec![Value::local("Agent::run".to_string())])?
            .success()
    );
    assert_eq!(demo.evidence.lock().unwrap()[0].as_ref().unwrap().len(), 1);
    Ok(())
}

#[allow(dead_code)]
fn main() -> Result<(), String> {
    run()
}
