use phenix_core::{
    Authority, Bytes, CapabilityGenerationId, Kernel, KernelConfig, LocalPersistence, ModelId,
    PhenixValue, PluginId, Project, ResolvedHarness, ResolvedHarnessActivation, RoutingProfileId,
};
use phenix_plugin_context::{context_component_manifest, context_factory, context_manifest};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
    execution_resource_service, step_attempt_service,
};
use phenix_plugin_models::{
    model_routing_component_manifest, model_routing_factory, model_routing_manifest,
    model_routing_service,
};
use phenix_sdk::{
    context_service, execution_service, AttemptOutcome, BudgetActual, BudgetReservation,
    BudgetReservationPurpose, BudgetReservationRequest, CachePlacement, CapacityKnowledge,
    ContextAdmissionRequest, ContextCandidate, ContextCommand, ContextControl, ContextDemand,
    ContextProjectionForm, ContextResponse, ContextRetention, ContextSource,
    DelegationResourcePolicy, EffectiveModelCapabilities, ExecutionAuthority, ExecutionCommand,
    ExecutionResourceCommand, ExecutionResourceResponse, ModelCommand, ModelLimits, ModelResponse,
    ModelTarget, ProjectionRevision, ReasoningBudget, RetryBudget, RootBudgetLedger,
    RootBudgetLimits, RouteEligibility, RouteSelectionPolicy, RoutingEstimateMode, RoutingProfile,
    RoutingRequirements, SkillProvisionBudget, StepAttemptCommand, StepAttemptPhase,
    StepAttemptResponse, StepPlan, ToolProvisionBudget, UsageAttemptKind, UsageAttribution,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const ROOT: &str = "root";
const ATTEMPT: &str = "attempt-1";
const RESERVATION: &str = "reservation-attempt-1";

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-coordination-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn authority() -> Authority {
    context_manifest().maximum_authority
}

fn kernel(path: &PathBuf) -> Kernel {
    let authority = authority();
    let context = context_manifest();
    let context_id = context.id.clone();
    let execution = execution_manifest(authority.clone());
    let execution_id = execution.id.clone();
    let models = model_routing_manifest(authority.clone());
    let models_id = models.id.clone();
    let resolved = ResolvedHarness::resolve(
        [execution.clone(), context.clone(), models.clone()],
        [
            execution_component_manifest(authority.clone()),
            context_component_manifest(),
            model_routing_component_manifest(authority.clone()),
        ],
        [],
        &authority,
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(
        KernelConfig::new([execution, context, models]).unwrap(),
        persistence,
    );
    kernel.activate_resolved_harness(&resolved).unwrap();
    kernel
        .register_embedded_factory(execution_id, execution_factory)
        .unwrap();
    kernel
        .register_embedded_factory(context_id, context_factory)
        .unwrap();
    kernel
        .register_embedded_factory(models_id, model_routing_factory)
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke_execution(kernel: &mut Kernel, command: ExecutionCommand) {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    kernel
        .invoke(&execution_service(), &input, &authority(), None)
        .unwrap();
}

fn invoke_resources(
    kernel: &mut Kernel,
    command: ExecutionResourceCommand,
) -> Result<ExecutionResourceResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&execution_resource_service(), &input, &authority(), None)
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ExecutionResourceResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn invoke_attempt(
    kernel: &mut Kernel,
    command: StepAttemptCommand,
) -> Result<StepAttemptResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&step_attempt_service(), &input, &authority(), None)
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    StepAttemptResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn invoke_models(kernel: &mut Kernel, command: ModelCommand) -> Result<ModelResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&model_routing_service(), &input, &authority(), None)
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ModelResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn invoke_context(kernel: &mut Kernel, command: ContextCommand) -> Result<ContextResponse, String> {
    let input = serde_json::to_vec(&PhenixValue::from(&command)).unwrap();
    let output = kernel
        .invoke(&context_service(), &input, &authority(), None)
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    ContextResponse::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn plan() -> StepPlan {
    let context = ContextDemand {
        mandatory_input_tokens: 500,
        reducible_input_tokens: 250,
        output_reserve_tokens: 128,
        required_capabilities: BTreeSet::new(),
    };
    StepPlan {
        policy_revision: "policy-1".into(),
        historical_estimator_snapshot: None,
        routing: RoutingRequirements {
            context: context.clone(),
            required_capabilities: BTreeSet::new(),
            require_known_capacity: true,
        },
        context,
        reasoning: ReasoningBudget::BackendDefault,
        tools: ToolProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_schemas: 4,
            max_result_bytes: 64 * 1024,
        },
        skills: SkillProvisionBudget {
            initial: BTreeSet::new(),
            expandable: BTreeSet::new(),
            max_loaded: 4,
        },
        delegation: DelegationResourcePolicy::default(),
        retry: RetryBudget {
            max_attempts: 2,
            reserved_attempts: 2,
        },
        reservation: BudgetReservation {
            input_tokens: 750,
            output_tokens: 128,
            cost_microunits: Some(1_000),
        },
        deadline_at_ms: None,
        reducible_input_dropped_tokens: 0,
    }
}

fn root_budget() -> RootBudgetLedger {
    RootBudgetLedger {
        root_execution_id: ROOT.into(),
        limits: RootBudgetLimits {
            fresh_input_tokens: 10_000,
            output_tokens: 2_000,
            cost_microunits: Some(10_000),
            attempts: 4,
        },
        reservations: BTreeMap::new(),
    }
}

fn reservation(plan: &StepPlan) -> BudgetReservationRequest {
    BudgetReservationRequest {
        reservation_id: RESERVATION.into(),
        parent_reservation_id: None,
        policy_revision: plan.policy_revision.clone(),
        purpose: BudgetReservationPurpose::RootStep,
        budget: plan.reservation.clone(),
        attempts: plan.retry.reserved_attempts,
    }
}

fn attribution(plan: &StepPlan) -> UsageAttribution {
    UsageAttribution {
        root_execution_id: ROOT.into(),
        execution_id: ROOT.into(),
        attempt_id: ATTEMPT.into(),
        parent_attempt_id: None,
        policy_revision: plan.policy_revision.clone(),
        kind: UsageAttemptKind::Root,
        task_id: None,
    }
}

fn target(provider: &str, model: &str) -> ModelTarget {
    ModelTarget {
        provider_plugin: PluginId::parse(provider).unwrap(),
        model: ModelId::parse(model).unwrap(),
        options: BTreeMap::new(),
    }
}

fn capabilities(target: ModelTarget, context_window_tokens: u64) -> EffectiveModelCapabilities {
    EffectiveModelCapabilities {
        target,
        generation: CapabilityGenerationId::parse("generation-1").unwrap(),
        context: ContextControl::ReplaceableTurns,
        capacity: CapacityKnowledge::Known {
            limits: ModelLimits {
                context_window_tokens,
                max_output_tokens: Some(512),
            },
        },
        cache: Default::default(),
        optional: BTreeSet::new(),
    }
}

fn prepare_root(kernel: &mut Kernel, plan: &StepPlan) {
    invoke_execution(
        kernel,
        ExecutionCommand::CreateExecution {
            id: ROOT.into(),
            requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
        },
    );
    invoke_resources(
        kernel,
        ExecutionResourceCommand::RegisterRootBudget {
            ledger: root_budget(),
        },
    )
    .unwrap();
    invoke_attempt(
        kernel,
        StepAttemptCommand::Create {
            attribution: attribution(plan),
            plan: plan.clone(),
        },
    )
    .unwrap();
}

fn reserve_attempt(kernel: &mut Kernel, plan: &StepPlan) {
    invoke_resources(
        kernel,
        ExecutionResourceCommand::Reserve {
            root_execution_id: ROOT.into(),
            reservation: reservation(plan),
        },
    )
    .unwrap();
    invoke_attempt(
        kernel,
        StepAttemptCommand::BindReservation {
            attempt_id: ATTEMPT.into(),
            reservation_id: RESERVATION.into(),
        },
    )
    .unwrap();
}

fn configure_routing(kernel: &mut Kernel, plan: &StepPlan) -> phenix_sdk::RouteDecision {
    let profile_id = RoutingProfileId::parse("default").unwrap();
    let small = target("provider.small", "small");
    let large = target("provider.large", "large");
    invoke_models(
        kernel,
        ModelCommand::RegisterProfile {
            profile: RoutingProfile {
                id: profile_id.clone(),
                default_target: small.clone(),
                fallback_targets: vec![large.clone()],
                callable_targets: BTreeMap::new(),
            },
        },
    )
    .unwrap();
    invoke_models(
        kernel,
        ModelCommand::PublishCapabilities {
            capabilities: capabilities(small.clone(), 700),
        },
    )
    .unwrap();
    invoke_models(
        kernel,
        ModelCommand::PublishCapabilities {
            capabilities: capabilities(large.clone(), 4_096),
        },
    )
    .unwrap();
    let response = invoke_models(
        kernel,
        ModelCommand::ResolveWithRequirements {
            profile_id,
            callable_id: None,
            requirements: plan.routing.clone(),
            policy: RouteSelectionPolicy {
                revision: "route-policy-1".into(),
                estimates: RoutingEstimateMode::Ignore,
                max_candidate_attempts: 8,
            },
        },
    )
    .unwrap();
    let ModelResponse::Decision { selection } = response else {
        panic!("expected route decision");
    };
    assert_eq!(selection.decision.target, large);
    assert!(selection.rejected.iter().any(|candidate| {
        candidate.target == small
            && candidate.reason == RouteEligibility::InsufficientContextCapacity
    }));
    selection.decision
}

fn bind_route(kernel: &mut Kernel, plan: &StepPlan) -> phenix_sdk::RouteDecision {
    let decision = configure_routing(kernel, plan);
    invoke_attempt(
        kernel,
        StepAttemptCommand::BindRoute {
            attempt_id: ATTEMPT.into(),
            decision: decision.clone(),
        },
    )
    .unwrap();
    decision
}

fn admit_context(kernel: &mut Kernel, plan: &StepPlan) -> ProjectionRevision {
    let response = invoke_context(
        kernel,
        ContextCommand::Admit {
            request: ContextAdmissionRequest {
                execution_id: ROOT.into(),
                step_plan: plan.clone(),
                candidates: vec![ContextCandidate {
                    id: "instruction-1".into(),
                    source: ContextSource::Inline {
                        identity: "instruction-1".into(),
                    },
                    content_identity: "sha256:instruction-1".into(),
                    content: Bytes::from(b"preserve the exact task constraints".to_vec()),
                    estimated_tokens: 500,
                    mandatory: true,
                    retention: ContextRetention::Pinned,
                    cache: CachePlacement::Epoch,
                    recovery: None,
                }],
                cache_epoch: 1,
            },
        },
    )
    .unwrap();
    let ContextResponse::Admission { result, projection } = response else {
        panic!("expected context admission");
    };
    assert_eq!(result.execution_id, ROOT);
    assert_eq!(result.admitted.len(), 1);
    assert_eq!(result.admitted[0].form, ContextProjectionForm::Full);
    invoke_attempt(
        kernel,
        StepAttemptCommand::BindProjection {
            attempt_id: ATTEMPT.into(),
            projection: projection.clone(),
        },
    )
    .unwrap();
    projection
}

fn lookup_attempt(kernel: &mut Kernel) -> phenix_sdk::StepAttemptRecord {
    let response = invoke_attempt(
        kernel,
        StepAttemptCommand::Get {
            attempt_id: ATTEMPT.into(),
        },
    )
    .unwrap();
    let StepAttemptResponse::AttemptLookup {
        attempt: Some(attempt),
    } = response
    else {
        panic!("expected persisted attempt");
    };
    attempt
}

mod budget_join {
    use super::*;

    #[test]
    fn root_reservation_and_attempt_use_the_same_policy_budget_and_identity() {
        let path = temp_db("budget");
        let mut kernel = kernel(&path);
        let plan = plan();
        prepare_root(&mut kernel, &plan);
        reserve_attempt(&mut kernel, &plan);

        let remaining = invoke_resources(
            &mut kernel,
            ExecutionResourceCommand::Remaining {
                root_execution_id: ROOT.into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 9_250);
        assert_eq!(budget.output_tokens, 1_872);
        assert_eq!(budget.cost_microunits, Some(9_000));
        assert_eq!(budget.attempts, 2);

        let attempt = lookup_attempt(&mut kernel);
        assert_eq!(attempt.phase, StepAttemptPhase::Reserved);
        assert_eq!(attempt.reservation_id.as_deref(), Some(RESERVATION));
        assert_eq!(attempt.plan.policy_revision, plan.policy_revision);
        let _ = fs::remove_file(path);
    }
}

mod routing_join {
    use super::*;

    #[test]
    fn hard_capacity_filter_selects_fallback_and_attempt_pins_exact_decision() {
        let path = temp_db("routing");
        let mut kernel = kernel(&path);
        let plan = plan();
        prepare_root(&mut kernel, &plan);
        reserve_attempt(&mut kernel, &plan);
        let decision = bind_route(&mut kernel, &plan);

        let attempt = lookup_attempt(&mut kernel);
        assert_eq!(attempt.phase, StepAttemptPhase::Routed);
        assert_eq!(attempt.route.as_ref(), Some(&decision));
        assert_eq!(decision.target.model.as_str(), "large");
        let _ = fs::remove_file(path);
    }
}

mod context_join {
    use super::*;

    #[test]
    fn context_admission_returns_the_revision_pinned_by_the_attempt() {
        let path = temp_db("context");
        let mut kernel = kernel(&path);
        let plan = plan();
        prepare_root(&mut kernel, &plan);
        reserve_attempt(&mut kernel, &plan);
        bind_route(&mut kernel, &plan);
        let projection = admit_context(&mut kernel, &plan);

        assert_eq!(projection.revision, 1);
        assert_eq!(projection.cache_epoch, 1);
        let attempt = lookup_attempt(&mut kernel);
        assert_eq!(attempt.phase, StepAttemptPhase::ContextAdmitted);
        assert_eq!(attempt.projection.as_ref(), Some(&projection));
        let _ = fs::remove_file(path);
    }
}

mod terminal_accounting_join {
    use super::*;

    #[test]
    fn settled_attempt_and_root_budget_report_the_same_completed_work() {
        let path = temp_db("terminal");
        let mut kernel = kernel(&path);
        let plan = plan();
        prepare_root(&mut kernel, &plan);
        reserve_attempt(&mut kernel, &plan);
        let decision = bind_route(&mut kernel, &plan);
        let projection = admit_context(&mut kernel, &plan);
        invoke_attempt(
            &mut kernel,
            StepAttemptCommand::MarkDispatched {
                attempt_id: ATTEMPT.into(),
                dispatch_id: "dispatch-1".into(),
            },
        )
        .unwrap();
        invoke_resources(
            &mut kernel,
            ExecutionResourceCommand::SettleReservation {
                root_execution_id: ROOT.into(),
                reservation_id: RESERVATION.into(),
                actual: BudgetActual {
                    fresh_input_tokens: 500,
                    output_tokens: 100,
                    cost_microunits: Some(400),
                    attempts: 1,
                },
            },
        )
        .unwrap();
        invoke_attempt(
            &mut kernel,
            StepAttemptCommand::Settle {
                attempt_id: ATTEMPT.into(),
                outcome: AttemptOutcome::Succeeded,
            },
        )
        .unwrap();

        let attempt = lookup_attempt(&mut kernel);
        assert_eq!(attempt.phase, StepAttemptPhase::Settled);
        assert_eq!(attempt.reservation_id.as_deref(), Some(RESERVATION));
        assert_eq!(attempt.route.as_ref(), Some(&decision));
        assert_eq!(attempt.projection.as_ref(), Some(&projection));
        assert_eq!(attempt.dispatch_id.as_deref(), Some("dispatch-1"));
        assert_eq!(attempt.outcome, Some(AttemptOutcome::Succeeded));

        let remaining = invoke_resources(
            &mut kernel,
            ExecutionResourceCommand::Remaining {
                root_execution_id: ROOT.into(),
            },
        )
        .unwrap();
        let ExecutionResourceResponse::Remaining { budget } = remaining else {
            panic!("expected remaining budget");
        };
        assert_eq!(budget.fresh_input_tokens, 9_500);
        assert_eq!(budget.output_tokens, 1_900);
        assert_eq!(budget.cost_microunits, Some(9_600));
        assert_eq!(budget.attempts, 3);
        let _ = fs::remove_file(path);
    }
}
