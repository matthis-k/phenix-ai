from pathlib import Path


def replace_once(path, old, new):
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}: {old[:100]!r}")
    file.write_text(text.replace(old, new, 1))


registry = "rust/crates/phenix-core/src/registry.rs"
replace_once(
    registry,
    '''    Persistence {
        plugin: PluginId,
        message: String,
    },
    EmbeddedFactoryMissing(PluginId),''',
    '''    Persistence {
        plugin: PluginId,
        message: String,
    },
    PersistenceConflict {
        plugin: PluginId,
        namespace: ResourceNamespace,
        key: String,
    },
    EmbeddedFactoryMissing(PluginId),''',
)
replace_once(
    registry,
    '''            Self::Persistence { plugin, message } => {
                write!(f, "plugin {plugin} persistence operation failed: {message}")
            }
            Self::EmbeddedFactoryMissing(plugin) => {''',
    '''            Self::Persistence { plugin, message } => {
                write!(f, "plugin {plugin} persistence operation failed: {message}")
            }
            Self::PersistenceConflict {
                plugin,
                namespace,
                key,
            } => write!(
                f,
                "plugin {plugin} persistence assertion conflicted at {namespace}/{key}"
            ),
            Self::EmbeddedFactoryMissing(plugin) => {''',
)

runtime = "rust/crates/phenix-core/src/runtime.rs"
replace_once(
    runtime,
    "    KernelConfig, KernelError, KernelEvent, KernelPolicyIdentity, LocalPersistence,\n    PersistenceBackend, PluginArtifact, PluginExecution, PluginId, PluginManifest,",
    "    KernelConfig, KernelError, KernelEvent, KernelPolicyIdentity, LocalPersistence,\n    PersistenceBackend, PersistenceError, PluginArtifact, PluginExecution, PluginId, PluginManifest,",
)

host = "rust/crates/phenix-core/src/runtime/host.rs"
file = Path(host)
text = file.read_text()
old = ".map_err(|error| self.persistence_error(error.to_string()))"
count = text.count(old)
if count != 6:
    raise SystemExit(f"{host}: expected six backend persistence mappings, found {count}")
text = text.replace(old, ".map_err(|error| self.persistence_error(error))")
old_prepare = ".map_err(|message| self.persistence_error(message))"
if text.count(old_prepare) != 1:
    raise SystemExit(f"{host}: prepared mutation mapping changed")
text = text.replace(old_prepare, ".map_err(|message| self.persistence_message(message))", 1)
old_helper = '''    fn persistence_error(&self, message: String) -> KernelError {
        KernelError::Persistence {
            plugin: self.plugin.clone(),
            message,
        }
    }
'''
new_helper = '''    fn persistence_error(&self, error: PersistenceError) -> KernelError {
        match error {
            PersistenceError::AssertionFailed { namespace, key } => {
                KernelError::PersistenceConflict {
                    plugin: self.plugin.clone(),
                    namespace,
                    key,
                }
            }
            error => self.persistence_message(error.to_string()),
        }
    }

    fn persistence_message(&self, message: String) -> KernelError {
        KernelError::Persistence {
            plugin: self.plugin.clone(),
            message,
        }
    }
'''
if text.count(old_helper) != 1:
    raise SystemExit(f"{host}: persistence helper changed")
file.write_text(text.replace(old_helper, new_helper, 1))

tests = "rust/crates/phenix-core/src/runtime/tests.rs"
file = Path(tests)
text = file.read_text()
anchor = '''#[test]
fn prepared_transaction_requires_write_authority_on_foreign_typed_import() {'''
if text.count(anchor) != 1:
    raise SystemExit("runtime persistence test anchor changed")
regression = r'''#[test]
fn persistence_assertion_conflict_preserves_namespace_and_key() {
    let namespace = ResourceNamespace::parse("conflict.state").unwrap();
    let owner = plugin("conflict-owner");
    let manifest = PluginManifest {
        id: owner.clone(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: Vec::new(),
        resource_namespaces: vec![namespace.clone()],
        maximum_authority: Authority::new([capability(PERSISTENCE_WRITE)]),
    };
    let mut kernel = Kernel::new(KernelConfig::new([manifest]).unwrap());
    kernel
        .persistence
        .lock()
        .register_schema(&owner, &DurableSchema::new(namespace.clone(), 1))
        .unwrap();
    let authority = Authority::new([capability(PERSISTENCE_WRITE)]);
    let prepared_mutations = PreparedMutationScope::new(kernel.graph_generation());
    let host = PluginHost {
        graph_generation: kernel.graph_generation(),
        component_graph: kernel.component_graph(),
        config: kernel.config(),
        states: &kernel.states,
        instances: &kernel.instances,
        plugin: &owner,
        authority: &authority,
        call_cancellation: None,
        call_stack: BTreeSet::from([owner.clone()]),
        events: &kernel.events,
        tasks: &kernel.tasks,
        persistence: &kernel.persistence,
        prepared_mutations: &prepared_mutations,
        provenance: &kernel.provenance,
        continuation: None,
        active_services: BTreeSet::new(),
        active_component_endpoints: BTreeSet::new(),
    };

    host.transact_durable(
        &namespace,
        &[TransactionOp::Put {
            key: "tail".into(),
            value: b"1".to_vec(),
        }],
    )
    .unwrap();
    let error = host
        .transact_durable(
            &namespace,
            &[TransactionOp::AssertValue {
                key: "tail".into(),
                expected: Some(b"0".to_vec()),
            }],
        )
        .unwrap_err();

    assert_eq!(
        error,
        KernelError::PersistenceConflict {
            plugin: owner,
            namespace,
            key: "tail".into(),
        }
    );
}

'''
file.write_text(text.replace(anchor, regression + anchor, 1))
