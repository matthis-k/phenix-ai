use phenix_core::{
    Authority, BackendFeature, CapabilityGenerationId, DurableSchema, Kernel, KernelConfig,
    LocalPersistence, ModelId, NamespaceTransaction, PersistenceBackend, PersistenceError,
    PhenixValue, PluginId, Project, ResourceNamespace, SchemaMigration, ServiceId, ValueError,
};
use phenix_plugin_execution::{
    execution_factory, execution_manifest, execution_resource_service, step_attempt_service,
    step_transaction_service,
};
use phenix_sdk::{
    AttemptOutcome, BudgetActual, BudgetReservation, BudgetReservationPurpose,
    BudgetReservationRequest, ContextDemand, DelegationResourcePolicy, ExecutionResourceCommand,
    ExecutionResourceResponse, ModelTarget, ProjectionRevision, ReasoningBudget, RetryBudget,
    RootBudgetLedger, RootBudgetLimits, RouteDecision, RoutingRequirements, SkillProvisionBudget,
    StepAttemptCommand, StepAttemptPhase, StepAttemptResponse, StepPlan, StepTransactionCommand,
    StepTransactionResponse, ToolProvisionBudget, UsageAttemptKind, UsageAttribution,
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

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-step-transaction-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn authority() -> Authority {
    execution_manifest(Authority::default()).maximum_authority
}

struct FailFirstMultiTransaction {
    inner: LocalPersistence,
    fail: Arc<AtomicBool>,
}

impl PersistenceBackend for FailFirstMultiTransaction {
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
        if transactions.len() > 1 && self.fail.swap(false, Ordering::SeqCst) {
            return Err(PersistenceError::AssertionFailed {
                namespace: transactions[0].namespace.clone(),
                key: "injected-multi-transaction-failure".into(),
            });
        }
        self.inner.transact_many(transactions)
    }
}

fn kernel(path: &PathBuf) -> Kernel {
    let manifest = execution_manifest(Authority::default());
    let plugin = manifest.id.clone();
    let fail = Arc::new(AtomicBool::new(true));
    let persistence = FailFirstMultiTransaction {
        inner: LocalPersistence::open(path).unwrap(),
        fail,
    };
    let mut kernel = Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
    kernel
        .register_embedded_factory(plugin, execution_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke<C, R>(kernel: &mut Kernel, service: ServiceId, command: &C) -> Result<R, String>
where
    for<'value> PhenixValue: From<&'value C>,
    for<'value> R: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let output = kernel
        .invoke(
            &service,
            &serde_json::to_vec(&PhenixValue::from(command)).unwrap(),
            &authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    R::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn plan() -> StepPlan {
    let context = ContextDemand {
        mandatory_input_tokens: 100,
        reducible_input_tokens: 0,
        output_reserve_tokens: 50,
        required_capabilities: BTreeSet::new(),
    };
    StepPlan {
        policy_revision: "policy-1".into(),
        routing: RoutingRequirements {
            context: context.clone(),
            required_capabilities: BTreeSet::new(),
            require_known_capacity: false,
        },
        context,
        reasoning: ReasoningBudget::BackendDefault,
        tools: ToolProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_schemas: 0,
            max_result_bytes: 0,
        },
        skills: SkillProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_loaded: 0,
        },
        delegation: DelegationResourcePolicy::default(),
        retry: RetryBudget {
            max_attempts: 1,
            reserved_attempts: 1,
        },
        reservation: BudgetReservation {
            input_tokens: 100,
            output_tokens: 50,
            cost_microunits: Some(500),
        },
        deadline_at_ms: None,
        reducible_input_dropped_tokens: 0,
    }
}

fn route() -> RouteDecision {
    RouteDecision {
        target: ModelTarget {
            provider_plugin: PluginId::parse("provider.fixture").unwrap(),
            model: ModelId::parse("model.fixture").unwrap(),
            options: BTreeMap::new(),
        },
        capability_generation: CapabilityGenerationId::parse("generation-1").unwrap(),
        policy_revision: "route-policy-1".into(),
        candidate_ordinal: 0,
        estimate: None,
    }
}

fn setup_dispatched(kernel: &mut Kernel) {
    let _: ExecutionResourceResponse = invoke(
        kernel,
        execution_resource_service(),
        &ExecutionResourceCommand::RegisterRootBudget {
            ledger: RootBudgetLedger {
                root_execution_id: "root".into(),
                limits: RootBudgetLimits {
                    fresh_input_tokens: 1_000,
                    output_tokens: 500,
                    cost_microunits: Some(5_000),
                    attempts: 4,
                },
                reservations: BTreeMap::new(),
            },
        },
    )
    .unwrap();
    let _: ExecutionResourceResponse = invoke(
        kernel,
        execution_resource_service(),
        &ExecutionResourceCommand::Reserve {
            root_execution_id: "root".into(),
            reservation: BudgetReservationRequest {
                reservation_id: "reservation-1".into(),
                parent_reservation_id: None,
                policy_revision: "policy-1".into(),
                purpose: BudgetReservationPurpose::RootStep,
                budget: plan().reservation,
                attempts: 1,
            },
        },
    )
    .unwrap();

    let attribution = UsageAttribution {
        root_execution_id: "root".into(),
        execution_id: "root".into(),
        attempt_id: "attempt-1".into(),
        parent_attempt_id: None,
        policy_revision: "policy-1".into(),
        kind: UsageAttemptKind::Root,
        task_id: None,
    };
    let _: StepAttemptResponse = invoke(
        kernel,
        step_attempt_service(),
        &StepAttemptCommand::Create {
            attribution,
            plan: plan(),
        },
    )
    .unwrap();
    for command in [
        StepAttemptCommand::BindReservation {
            attempt_id: "attempt-1".into(),
            reservation_id: "reservation-1".into(),
        },
        StepAttemptCommand::BindRoute {
            attempt_id: "attempt-1".into(),
            decision: route(),
        },
        StepAttemptCommand::BindProjection {
            attempt_id: "attempt-1".into(),
            projection: ProjectionRevision {
                revision: 1,
                cache_epoch: 1,
            },
        },
        StepAttemptCommand::MarkDispatched {
            attempt_id: "attempt-1".into(),
            dispatch_id: "dispatch-1".into(),
        },
    ] {
        let _: StepAttemptResponse = invoke(kernel, step_attempt_service(), &command).unwrap();
    }
}

fn actual() -> BudgetActual {
    BudgetActual {
        fresh_input_tokens: 100,
        output_tokens: 50,
        cost_microunits: Some(500),
        attempts: 1,
    }
}

fn settle(kernel: &mut Kernel) -> Result<StepTransactionResponse, String> {
    invoke(
        kernel,
        step_transaction_service(),
        &StepTransactionCommand::Settle {
            root_execution_id: "root".into(),
            reservation_id: "reservation-1".into(),
            actual: actual(),
            attempt_id: "attempt-1".into(),
            outcome: AttemptOutcome::Succeeded,
        },
    )
}

fn attempt(kernel: &mut Kernel) -> phenix_sdk::StepAttemptRecord {
    let response: StepAttemptResponse = invoke(
        kernel,
        step_attempt_service(),
        &StepAttemptCommand::Get {
            attempt_id: "attempt-1".into(),
        },
    )
    .unwrap();
    let StepAttemptResponse::AttemptLookup {
        attempt: Some(attempt),
    } = response
    else {
        panic!("attempt must exist");
    };
    attempt
}

#[test]
fn failed_atomic_settlement_commits_neither_owner() {
    let path = temp_db("failure");
    let mut kernel = kernel(&path);
    setup_dispatched(&mut kernel);

    assert!(settle(&mut kernel).is_err());
    assert_eq!(attempt(&mut kernel).phase, StepAttemptPhase::Dispatched);

    // The reservation must still be active if the resource half did not commit.
    let response: ExecutionResourceResponse = invoke(
        &mut kernel,
        execution_resource_service(),
        &ExecutionResourceCommand::SettleReservation {
            root_execution_id: "root".into(),
            reservation_id: "reservation-1".into(),
            actual: actual(),
        },
    )
    .unwrap();
    assert!(matches!(response, ExecutionResourceResponse::RootBudget { .. }));
    drop(kernel);
    let _ = fs::remove_file(path);
}

#[test]
fn failed_atomic_settlement_can_be_retried_as_one_transaction() {
    let path = temp_db("retry");
    let mut kernel = kernel(&path);
    setup_dispatched(&mut kernel);

    assert!(settle(&mut kernel).is_err());
    let response = settle(&mut kernel).unwrap();
    let StepTransactionResponse::Settled { attempt, .. } = response;
    assert_eq!(attempt.phase, StepAttemptPhase::Settled);
    assert_eq!(attempt.outcome, Some(AttemptOutcome::Succeeded));
    assert_eq!(attempt(&mut kernel).phase, StepAttemptPhase::Settled);
    drop(kernel);
    let _ = fs::remove_file(path);
}
