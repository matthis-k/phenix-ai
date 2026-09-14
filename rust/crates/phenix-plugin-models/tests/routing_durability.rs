use phenix_core::{
    Authority, BackendFeature, CapabilityGenerationId, DurableSchema, Kernel, KernelConfig,
    LocalPersistence, ModelId, NamespaceTransaction, PersistenceBackend, PersistenceError,
    PhenixValue, PluginId, Project, ResourceNamespace, SchemaMigration,
};
use phenix_plugin_models::{
    model_routing_factory, model_routing_manifest, model_routing_service, ModelCommand,
    ModelResponse, ModelTarget, RoutingProfile,
};
use phenix_sdk::{
    CapacityKnowledge, ContextControl, EffectiveModelCapabilities, ModelLimits, ModelTurnUsage,
    RouteDecision, RoutingEvidence,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const ROUTING_NAMESPACE: &str = "phenix.models.state";
const ROUTING_RUNTIME_KEY: &str = "runtime/routing-state";

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-routing-durability-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn target() -> ModelTarget {
    ModelTarget {
        provider_plugin: PluginId::parse("provider.fixture").unwrap(),
        model: ModelId::parse("model.fixture").unwrap(),
        options: BTreeMap::new(),
    }
}

fn profile() -> RoutingProfile {
    RoutingProfile {
        id: phenix_core::RoutingProfileId::parse("default").unwrap(),
        default_target: target(),
        fallback_targets: Vec::new(),
        callable_targets: BTreeMap::new(),
    }
}

fn capabilities(generation: &str) -> EffectiveModelCapabilities {
    EffectiveModelCapabilities {
        target: target(),
        generation: CapabilityGenerationId::parse(generation).unwrap(),
        context: ContextControl::ReplaceableTurns,
        capacity: CapacityKnowledge::Known {
            limits: ModelLimits {
                context_window_tokens: 8_000,
                max_output_tokens: Some(1_000),
            },
        },
        optional: BTreeSet::new(),
    }
}

fn decision(generation: &str) -> RouteDecision {
    RouteDecision {
        target: target(),
        capability_generation: CapabilityGenerationId::parse(generation).unwrap(),
        policy_revision: "route-policy-1".into(),
        candidate_ordinal: 0,
        estimate: None,
    }
}

fn evidence(latency_ms: u64) -> RoutingEvidence {
    RoutingEvidence {
        success: true,
        latency_ms: Some(latency_ms),
        usage: ModelTurnUsage::default(),
    }
}

fn authority() -> Authority {
    model_routing_manifest(Authority::default()).maximum_authority
}

fn kernel_with(persistence: impl PersistenceBackend + 'static) -> Kernel {
    let manifest = model_routing_manifest(Authority::default());
    let plugin = manifest.id.clone();
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, model_routing_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke(kernel: &mut Kernel, command: ModelCommand) -> Result<ModelResponse, String> {
    let output = kernel
        .invoke(
            &model_routing_service(),
            &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
            &authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ModelResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

#[test]
fn recorded_evidence_survives_plugin_restart() {
    let path = temp_db("evidence-restart");
    {
        let mut kernel = kernel_with(LocalPersistence::open(&path).unwrap());
        invoke(
            &mut kernel,
            ModelCommand::RecordEvidence {
                decision: decision("generation-1"),
                evidence: evidence(10),
            },
        )
        .unwrap();
    }
    {
        let mut restored = kernel_with(LocalPersistence::open(&path).unwrap());
        invoke(
            &mut restored,
            ModelCommand::RecordEvidence {
                decision: decision("generation-1"),
                evidence: evidence(20),
            },
        )
        .unwrap();
    }

    let store = LocalPersistence::open(&path).unwrap();
    let raw = store
        .read(
            &PluginId::parse("phenix.models").unwrap(),
            &ResourceNamespace::parse(ROUTING_NAMESPACE).unwrap(),
            ROUTING_RUNTIME_KEY,
        )
        .unwrap()
        .expect("routing runtime snapshot exists");
    let snapshot: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let evidence = snapshot["evidence"].as_object().expect("evidence map");
    let sample_count: usize = evidence
        .values()
        .map(|samples| samples.as_array().expect("evidence samples").len())
        .sum();
    assert_eq!(sample_count, 2);
    drop(store);
    let _ = fs::remove_file(path);
}

struct FailNextPersistence {
    inner: LocalPersistence,
    fail_next: Arc<AtomicBool>,
}

impl PersistenceBackend for FailNextPersistence {
    fn supported_features(&self) -> BTreeSet<BackendFeature> {
        self.inner.supported_features()
    }

    fn register_schema(
        &mut self,
        owner: &PluginId,
        schema: &DurableSchema,
    ) -> Result<(), PersistenceError> {
        self.inner.register_schema(owner, schema)
    }

    fn migrate_schema(
        &mut self,
        owner: &PluginId,
        schema: &DurableSchema,
        migrations: &[SchemaMigration],
    ) -> Result<(), PersistenceError> {
        self.inner.migrate_schema(owner, schema, migrations)
    }

    fn read(
        &self,
        caller: &PluginId,
        namespace: &ResourceNamespace,
        key: &str,
    ) -> Result<Option<Vec<u8>>, PersistenceError> {
        self.inner.read(caller, namespace, key)
    }

    fn transact_many(
        &mut self,
        transactions: &[NamespaceTransaction],
    ) -> Result<(), PersistenceError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            let namespace = transactions
                .first()
                .map(|transaction| transaction.namespace.clone())
                .unwrap_or_else(|| ResourceNamespace::parse(ROUTING_NAMESPACE).unwrap());
            return Err(PersistenceError::AssertionFailed {
                namespace,
                key: "injected-failure".into(),
            });
        }
        self.inner.transact_many(transactions)
    }
}

#[test]
fn failed_runtime_persistence_rolls_back_in_memory_state() {
    let path = temp_db("rollback");
    let fail_next = Arc::new(AtomicBool::new(false));
    let persistence = FailNextPersistence {
        inner: LocalPersistence::open(&path).unwrap(),
        fail_next: fail_next.clone(),
    };
    let mut kernel = kernel_with(persistence);

    invoke(
        &mut kernel,
        ModelCommand::RegisterProfile { profile: profile() },
    )
    .unwrap();
    invoke(
        &mut kernel,
        ModelCommand::PublishCapabilities {
            capabilities: capabilities("generation-1"),
        },
    )
    .unwrap();

    fail_next.store(true, Ordering::SeqCst);
    assert!(invoke(
        &mut kernel,
        ModelCommand::PublishCapabilities {
            capabilities: capabilities("generation-2"),
        },
    )
    .is_err());

    let response = invoke(
        &mut kernel,
        ModelCommand::ListCandidates {
            profile_id: phenix_core::RoutingProfileId::parse("default").unwrap(),
            callable_id: None,
        },
    )
    .unwrap();
    let ModelResponse::Candidates { candidates } = response else {
        panic!("expected candidates response");
    };
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].capabilities.generation.as_str(),
        "generation-1"
    );
    drop(kernel);
    let _ = fs::remove_file(path);
}
