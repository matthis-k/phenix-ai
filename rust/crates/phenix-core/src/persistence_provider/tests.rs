use super::*;
use crate::{DurableSchema, NamespaceTransaction, ResourceNamespace};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Probe {
    opened: bool,
    registered: usize,
    preparation: Option<(
        ResolvedPersistenceBootstrap,
        Option<ResolvedPersistenceBootstrap>,
    )>,
}

struct MockProvider {
    descriptor: PersistenceProviderDescriptor,
    probe: Arc<Mutex<Probe>>,
}

impl PersistenceProvider for MockProvider {
    fn descriptor(&self) -> PersistenceProviderDescriptor {
        self.descriptor.clone()
    }

    fn prepare(
        &mut self,
        plan: &ResolvedPersistenceBootstrap,
        active: Option<&ResolvedPersistenceBootstrap>,
    ) -> Result<Box<dyn PersistenceBackend>, PersistenceProviderError> {
        self.probe.lock().unwrap().preparation = Some((plan.clone(), active.cloned()));
        match &plan.transition {
            Some(PersistenceProviderTransition::Migration { operation })
            | Some(PersistenceProviderTransition::ExportImport { operation })
                if operation != "fixture.copy" =>
            {
                return Err(PersistenceProviderError::new(
                    "unknown transition operation",
                ));
            }
            _ => {}
        }
        self.probe.lock().unwrap().opened = true;
        Ok(Box::new(MockBackend {
            probe: Arc::clone(&self.probe),
            features: self.descriptor.supported_features.clone(),
        }))
    }
}

struct MockBackend {
    probe: Arc<Mutex<Probe>>,
    features: BTreeSet<BackendFeature>,
}

impl PersistenceBackend for MockBackend {
    fn supported_features(&self) -> BTreeSet<BackendFeature> {
        self.features.clone()
    }

    fn register_schema(
        &mut self,
        _owner: &PluginId,
        _schema: &DurableSchema,
    ) -> Result<(), PersistenceError> {
        self.probe.lock().unwrap().registered += 1;
        Ok(())
    }

    fn read(
        &self,
        _caller: &PluginId,
        _namespace: &ResourceNamespace,
        _key: &str,
    ) -> Result<Option<Vec<u8>>, PersistenceError> {
        Ok(None)
    }

    fn transact_many(
        &mut self,
        _transactions: &[NamespaceTransaction],
    ) -> Result<(), PersistenceError> {
        Ok(())
    }
}

fn plugin(value: &str) -> PluginId {
    PluginId::parse(value).unwrap()
}

fn binding() -> StoreBinding {
    StoreBinding::new(crate::StoreBindingId::parse("fixture").unwrap(), "mock-v1").unwrap()
}

fn schema(feature: BackendFeature) -> DurableSchemaRegistration {
    DurableSchemaRegistration::new(
        plugin("fixture.owner"),
        DurableSchema::requiring(
            ResourceNamespace::parse("fixture.owner.state").unwrap(),
            1,
            [feature],
        ),
    )
}

#[test]
fn unsupported_feature_rejects_candidate_before_provider_opens_store() {
    let probe = Arc::new(Mutex::new(Probe::default()));
    let mut provider = MockProvider {
        descriptor: PersistenceProviderDescriptor::new(
            plugin("fixture.provider"),
            [BackendFeature::Transactions],
            ["mock-v1".to_owned()],
        ),
        probe: Arc::clone(&probe),
    };

    assert!(matches!(
        prepare_persistence_candidate(
            &mut provider,
            [],
            &BTreeSet::new(),
            binding(),
            &[schema(BackendFeature::IndexedRange)],
            None,
            None,
        ),
        Err(PersistenceCandidateError::Bootstrap(
            PersistenceBootstrapError::UnsupportedFeatures { .. }
        ))
    ));
    assert!(!probe.lock().unwrap().opened);
}

#[test]
fn eligible_candidate_opens_store_then_materializes_complete_schema_plan() {
    let probe = Arc::new(Mutex::new(Probe::default()));
    let mut provider = MockProvider {
        descriptor: PersistenceProviderDescriptor::new(
            plugin("fixture.provider"),
            [BackendFeature::Transactions],
            ["mock-v1".to_owned()],
        ),
        probe: Arc::clone(&probe),
    };

    let prepared = prepare_persistence_candidate(
        &mut provider,
        [],
        &BTreeSet::new(),
        binding(),
        &[schema(BackendFeature::Transactions)],
        None,
        None,
    )
    .unwrap();

    assert_eq!(prepared.plan().provider.plugin, plugin("fixture.provider"));
    let probe = probe.lock().unwrap();
    assert!(probe.opened);
    assert_eq!(probe.registered, 1);
}

fn prepare_transition(
    transition: PersistenceProviderTransition,
) -> (
    Result<PreparedPersistence, PersistenceCandidateError>,
    ResolvedPersistenceBootstrap,
    Arc<Mutex<Probe>>,
) {
    let active_provider = PersistenceProviderDescriptor::new(
        plugin("fixture.active"),
        [BackendFeature::Transactions],
        ["old-v1".to_owned()],
    );
    let active = resolve_persistence_bootstrap(
        &active_provider.plugin,
        [active_provider.clone()],
        &BTreeSet::new(),
        StoreBinding::new(binding().id, "old-v1").unwrap(),
        &[],
        None,
        None,
    )
    .unwrap();
    let probe = Arc::new(Mutex::new(Probe::default()));
    let mut provider = MockProvider {
        descriptor: PersistenceProviderDescriptor::new(
            plugin("fixture.candidate"),
            [BackendFeature::Transactions],
            ["mock-v1".to_owned()],
        ),
        probe: Arc::clone(&probe),
    };
    let result = prepare_persistence_candidate(
        &mut provider,
        [],
        &BTreeSet::new(),
        binding(),
        &[schema(BackendFeature::Transactions)],
        Some(&active),
        Some(transition),
    );
    (result, active, probe)
}

#[test]
fn provider_receives_transition_and_active_store_before_schema_preparation() {
    for transition in [
        PersistenceProviderTransition::Migration {
            operation: "fixture.copy".into(),
        },
        PersistenceProviderTransition::ExportImport {
            operation: "fixture.copy".into(),
        },
    ] {
        let (result, active, probe) = prepare_transition(transition.clone());
        let prepared = result.unwrap();
        let probe = probe.lock().unwrap();

        assert_eq!(
            probe.preparation.as_ref(),
            Some(&(prepared.plan().clone(), Some(active)))
        );
        assert_eq!(prepared.plan().transition, Some(transition));
        assert!(probe.opened);
        assert_eq!(probe.registered, 1);
    }
}

#[test]
fn rejected_transition_stops_before_store_open_and_schema_preparation() {
    let (result, active, probe) = prepare_transition(PersistenceProviderTransition::Migration {
        operation: "fixture.unknown".into(),
    });

    assert!(matches!(
        result,
        Err(PersistenceCandidateError::Provider { provider, error })
            if provider == plugin("fixture.candidate")
                && error.message == "unknown transition operation"
    ));
    let probe = probe.lock().unwrap();
    assert_eq!(
        probe.preparation.as_ref().unwrap().1.as_ref(),
        Some(&active)
    );
    assert_eq!(active.provider.plugin, plugin("fixture.active"));
    assert_eq!(active.binding.storage_format, "old-v1");
    assert!(!probe.opened);
    assert_eq!(probe.registered, 0);
}
