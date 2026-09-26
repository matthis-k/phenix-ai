use phenix_core::{
    ArtifactRevision, Authority, CapabilityGenerationId, ComponentInterface, InvocationOutcome,
    Kernel, KernelConfig, LocalPersistence, ModelId, ModelInferenceFailure, ModelInferenceRequest,
    ModelInferenceResponse, PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, Project, ResolvedHarness, ResolvedHarnessActivation,
    ServiceContribution, ServiceId, ValueError,
};
use phenix_plugin_context::{context_component_manifest, context_factory, context_manifest};
use phenix_plugin_execution::{
    execution_component_manifest, execution_factory, execution_manifest,
};
use phenix_plugin_models::{
    model_inference_service, model_routing_component_manifest, model_routing_factory,
    model_routing_manifest,
};
use phenix_plugin_step_runner::{
    step_runner_component_manifest, step_runner_factory, step_runner_manifest,
};
use phenix_sdk::{
    delegated_worker_service, execution_resource_service, execution_service, model_routing_service,
    step_attempt_service, step_runner_service, AttemptOutcome, BudgetReservation,
    BudgetReservationPurpose, BudgetReservationRequest, CapacityKnowledge, ContextCandidate,
    ContextControl, ContextDemand, ContextRetention, ContextSource, DelegatedWorkResources,
    DelegatedWorkerCommand, DelegatedWorkerResponse, DelegationResourcePolicy,
    DelegationTaskBinding, EffectiveModelCapabilities, ExecutionAuthority, ExecutionCommand,
    ExecutionResourceCommand, ExecutionResourceResponse, ExecutionResponse, ModelCommand,
    ModelLimits, ModelResponse, ModelTarget, PlannedStepRequest, RouteSelectionPolicy,
    RoutingEstimateMode, RoutingProfile, StepAttemptCommand, StepAttemptPhase, StepAttemptRecord,
    StepAttemptResponse, StepRunnerCommand, StepRunnerResponse, StepSettlementBasis,
    TaskRequirements, UsageAttemptKind, UsageAttribution, UsagePolicy, WorkerTaskRecord,
    WorkerTaskState,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct FixtureProvider;

impl PluginInstance for FixtureProvider {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &model_inference_service() {
            return Err(format!("unsupported fixture provider service: {service}"));
        }
        let context = PluginContext::new(host, (), (), ());
        let request = context
            .kernel
            .decode_projected::<ModelInferenceRequest>(
                &phenix_core::ModelInferenceInterface::interface_id(),
                input,
            )
            .map_err(|error| error.to_string())?;
        if request.input.as_ref() == b"provider-fails" {
            return Err("fixture provider failed after invocation".into());
        }
        if request.input.as_ref() == b"provider-unavailable" && request.model.as_str() == "small" {
            let failure = ModelInferenceFailure::Unavailable {
                message: "fixture provider is temporarily unavailable".into(),
            };
            let outcome =
                InvocationOutcome::domain_error(PhenixValue::from(&failure)).into_transport_value();
            return serde_json::to_vec(&outcome).map_err(|error| error.to_string());
        }
        context
            .kernel
            .encode_value(&ModelInferenceResponse {
                output: request.input,
                provider_metadata: BTreeMap::from([(
                    "model".into(),
                    serde_json::json!(request.model.as_str()).into(),
                )]),
                usage: Box::new(phenix_core::ModelTurnUsage {
                    output_tokens: phenix_core::UsageQuantity::Reported { value: 8 },
                    ..Default::default()
                }),
                tool_calls: Vec::new(),
            })
            .map_err(|error| error.to_string())
    }
}

fn temp_db(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "phenix-step-runner-{name}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn authority() -> Authority {
    Authority::new([
        phenix_core::CapabilityId::parse("kernel.persistence.schema").unwrap(),
        phenix_core::CapabilityId::parse("kernel.persistence.read").unwrap(),
        phenix_core::CapabilityId::parse("kernel.persistence.write").unwrap(),
    ])
}

fn provider_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse("fixture.provider").unwrap(),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: model_inference_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority: Authority::default(),
    }
}

fn kernel(path: &PathBuf) -> Kernel {
    let authority = authority();
    let execution = execution_manifest(authority.clone());
    let context = context_manifest();
    let models = model_routing_manifest(authority.clone());
    let runner = step_runner_manifest(authority.clone());
    let provider = provider_manifest();
    let execution_id = execution.id.clone();
    let context_id = context.id.clone();
    let models_id = models.id.clone();
    let runner_id = runner.id.clone();
    let provider_id = provider.id.clone();
    let resolved = ResolvedHarness::resolve(
        [
            execution.clone(),
            context.clone(),
            models.clone(),
            runner.clone(),
            provider.clone(),
        ],
        [
            execution_component_manifest(authority.clone()),
            context_component_manifest(),
            model_routing_component_manifest(authority.clone()),
            step_runner_component_manifest(authority.clone()),
        ],
        [],
        &authority,
    )
    .unwrap();
    let persistence = LocalPersistence::open(path).unwrap();
    let mut kernel = Kernel::with_persistence(
        KernelConfig::new([execution, context, models, runner, provider]).unwrap(),
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
    kernel
        .register_embedded_factory(runner_id, step_runner_factory)
        .unwrap();
    kernel
        .register_embedded_factory(provider_id, || Box::new(FixtureProvider))
        .unwrap();
    kernel.activate_all().unwrap();
    kernel
}

fn invoke<C, R>(kernel: &mut Kernel, service: ServiceId, command: &C) -> Result<R, String>
where
    for<'value> PhenixValue: From<&'value C>,
    for<'value> R: TryFrom<Project<&'value PhenixValue>, Error = ValueError>,
{
    let input = PhenixValue::from(command);
    let output = kernel
        .invoke(
            &service,
            &serde_json::to_vec(&input).unwrap(),
            &authority(),
            None,
        )
        .map_err(|error| error.to_string())?;
    let output: PhenixValue = serde_json::from_slice(&output).map_err(|error| error.to_string())?;
    R::try_from(Project(&output)).map_err(|error| error.to_string())
}

fn target(model: &str) -> ModelTarget {
    ModelTarget {
        provider_plugin: PluginId::parse("fixture.provider").unwrap(),
        model: ModelId::parse(model).unwrap(),
        options: BTreeMap::new(),
    }
}

fn capabilities(target: ModelTarget, window: u64) -> EffectiveModelCapabilities {
    EffectiveModelCapabilities {
        target,
        generation: CapabilityGenerationId::parse("generation-1").unwrap(),
        context: ContextControl::ReplaceableTurns,
        capacity: CapacityKnowledge::Known {
            limits: ModelLimits {
                context_window_tokens: window,
                max_output_tokens: Some(512),
            },
        },
        cache: Default::default(),
        optional: BTreeSet::new(),
    }
}

fn setup_root(kernel: &mut Kernel) {
    setup_root_with_output(kernel, 1_000);
}

fn setup_root_with_output(kernel: &mut Kernel, output_tokens: u64) {
    let _: ExecutionResponse = invoke(
        kernel,
        execution_service(),
        &ExecutionCommand::CreateExecution {
            id: "root".into(),
            requested_authority: ExecutionAuthority::new(Vec::<String>::new()),
        },
    )
    .unwrap();
    let _: ExecutionResourceResponse = invoke(
        kernel,
        execution_resource_service(),
        &ExecutionResourceCommand::RegisterRootBudget {
            ledger: phenix_sdk::RootBudgetLedger {
                root_execution_id: "root".into(),
                limits: phenix_sdk::RootBudgetLimits {
                    fresh_input_tokens: 4_000,
                    output_tokens,
                    cost_microunits: Some(10_000),
                    attempts: 4,
                },
                reservations: BTreeMap::new(),
            },
        },
    )
    .unwrap();
}

fn setup_routing(kernel: &mut Kernel, publish: bool, authenticate: bool) {
    let profile = RoutingProfile {
        id: phenix_core::RoutingProfileId::parse("default").unwrap(),
        default_target: target("small"),
        fallback_targets: vec![target("large")],
        callable_targets: BTreeMap::new(),
    };
    let _: ModelResponse = invoke(
        kernel,
        model_routing_service(),
        &ModelCommand::RegisterProfile {
            profile: profile.clone(),
        },
    )
    .unwrap();
    if publish {
        for capabilities in [
            capabilities(profile.default_target, 500),
            capabilities(profile.fallback_targets[0].clone(), 8_000),
        ] {
            let _: ModelResponse = invoke(
                kernel,
                model_routing_service(),
                &ModelCommand::PublishCapabilities { capabilities },
            )
            .unwrap();
        }
    }
    if authenticate {
        let _: ModelResponse = invoke(
            kernel,
            model_routing_service(),
            &ModelCommand::SetProviderAuthenticated {
                provider_plugin: PluginId::parse("fixture.provider").unwrap(),
                authenticated: true,
            },
        )
        .unwrap();
    }
}

fn policy(max_input: u64) -> UsagePolicy {
    UsagePolicy {
        revision: "policy-1".into(),
        max_fresh_input_tokens: max_input,
        max_output_tokens: 128,
        max_cost_microunits: Some(1_000),
        max_retries: 1,
        max_tool_result_bytes: 64 * 1024,
        max_tool_schemas: 4,
        max_skills: 4,
        require_known_capacity: true,
        delegation: DelegationResourcePolicy::default(),
    }
}

fn request(max_input: u64) -> PlannedStepRequest {
    PlannedStepRequest {
        attribution: UsageAttribution {
            root_execution_id: "root".into(),
            execution_id: "root".into(),
            attempt_id: "attempt-1".into(),
            parent_attempt_id: None,
            policy_revision: "policy-1".into(),
            kind: UsageAttemptKind::Root,
            task_id: None,
        },
        profile_id: phenix_core::RoutingProfileId::parse("default").unwrap(),
        callable_id: None,
        input: b"hello planned world".to_vec().into(),
        tools: Vec::new(),
        continuation: Vec::new(),
        policy: policy(max_input),
        task: TaskRequirements {
            request_input_tokens: 0,
            context: ContextDemand {
                mandatory_input_tokens: 600,
                reducible_input_tokens: 200,
                output_reserve_tokens: 128,
                required_capabilities: BTreeSet::new(),
            },
            required_capabilities: BTreeSet::new(),
            required_tools: BTreeSet::new(),
            optional_tools: BTreeSet::new(),
            required_skills: BTreeSet::new(),
            optional_skills: BTreeSet::new(),
            requested_reasoning: None,
            deadline_at_ms: None,
        },
        context_candidates: vec![ContextCandidate {
            id: "required".into(),
            source: ContextSource::Inline {
                identity: "required".into(),
            },
            content_identity: "sha256:required".into(),
            content: b"required context".to_vec().into(),
            estimated_tokens: 600,
            mandatory: true,
            retention: ContextRetention::Pinned,
            cache: phenix_sdk::CachePlacement::Epoch,
            recovery: None,
        }],
        cache_epoch: 1,
        route_policy: RouteSelectionPolicy {
            revision: "route-policy-1".into(),
            estimates: RoutingEstimateMode::Ignore,
            max_candidate_attempts: 4,
        },
        now_ms: 1_000,
    }
}

fn retry_request(
    attempt_id: &str,
    parent_attempt_id: &str,
    cache_epoch: u64,
) -> PlannedStepRequest {
    let mut request = request(1_000);
    request.attribution.attempt_id = attempt_id.into();
    request.attribution.parent_attempt_id = Some(parent_attempt_id.into());
    request.attribution.kind = UsageAttemptKind::Retry;
    request.cache_epoch = cache_epoch;
    request
}

fn lookup_attempt(kernel: &mut Kernel, attempt_id: &str) -> Option<StepAttemptRecord> {
    let response: StepAttemptResponse = invoke(
        kernel,
        step_attempt_service(),
        &StepAttemptCommand::Get {
            attempt_id: attempt_id.into(),
        },
    )
    .unwrap();
    match response {
        StepAttemptResponse::AttemptLookup { attempt } => attempt,
        other => panic!("unexpected attempt lookup: {other:?}"),
    }
}

fn remaining(kernel: &mut Kernel) -> phenix_sdk::RemainingBudget {
    let response: ExecutionResourceResponse = invoke(
        kernel,
        execution_resource_service(),
        &ExecutionResourceCommand::Remaining {
            root_execution_id: "root".into(),
        },
    )
    .unwrap();
    let ExecutionResourceResponse::Remaining { budget } = response else {
        panic!("expected remaining budget");
    };
    budget
}

mod planning_guard {
    use super::*;

    #[test]
    fn mandatory_overflow_fails_before_attempt_or_reservation() {
        let path = temp_db("planning-guard");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, true);
        let error = invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: request(500),
            },
        )
        .unwrap_err();
        assert!(error.contains("MandatoryInputExceedsBudget"));
        assert!(lookup_attempt(&mut kernel, "attempt-1").is_none());
        let remaining = remaining(&mut kernel);
        assert_eq!(remaining.fresh_input_tokens, 4_000);
        assert_eq!(remaining.attempts, 4);
        let _ = fs::remove_file(path);
    }
}

mod pre_dispatch_cleanup {
    use super::*;

    #[test]
    fn routing_failure_releases_reservation_and_aborts_attempt() {
        let path = temp_db("pre-dispatch");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, false, true);
        let error = invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: request(1_000),
            },
        )
        .unwrap_err();
        assert!(error.contains("MissingEffectiveCapabilities"));
        let attempt = lookup_attempt(&mut kernel, "attempt-1").expect("attempt was recorded");
        assert_eq!(attempt.phase, StepAttemptPhase::Settled);
        assert_eq!(attempt.outcome, Some(AttemptOutcome::Failed));
        assert_eq!(attempt.reservation_id.as_deref(), Some("attempt/attempt-1"));
        let remaining = remaining(&mut kernel);
        assert_eq!(remaining.fresh_input_tokens, 4_000);
        assert_eq!(remaining.output_tokens, 1_000);
        assert_eq!(remaining.cost_microunits, Some(10_000));
        assert_eq!(remaining.attempts, 4);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_authentication_fails_preflight_without_charging_provider_work() {
        let path = temp_db("preflight-auth");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, false);
        let error = invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: request(1_000),
            },
        )
        .unwrap_err();
        assert!(error.contains("authentication required"));
        let attempt = lookup_attempt(&mut kernel, "attempt-1").expect("attempt was recorded");
        assert_eq!(attempt.phase, StepAttemptPhase::Settled);
        assert_eq!(attempt.outcome, Some(AttemptOutcome::Failed));
        let remaining = remaining(&mut kernel);
        assert_eq!(remaining.fresh_input_tokens, 4_000);
        assert_eq!(remaining.output_tokens, 1_000);
        assert_eq!(remaining.cost_microunits, Some(10_000));
        assert_eq!(remaining.attempts, 4);
        let _ = fs::remove_file(path);
    }
}

mod successful_lifecycle {
    use super::*;

    #[test]
    fn smart_fallback_dispatches_and_settles_reported_output() {
        let path = temp_db("success");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, true);
        let response: StepRunnerResponse = invoke(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: request(1_000),
            },
        )
        .unwrap();
        let StepRunnerResponse::Completed {
            attempt,
            output,
            settled,
            settlement_basis,
            ..
        } = response;
        assert_eq!(attempt.phase, StepAttemptPhase::Settled);
        assert_eq!(attempt.outcome, Some(AttemptOutcome::Succeeded));
        assert_eq!(
            attempt.route.as_ref().unwrap().target.model.as_str(),
            "large"
        );
        assert_eq!(output.as_ref(), b"hello planned world");
        assert_eq!(
            settlement_basis,
            StepSettlementBasis::ProviderReportedOutput
        );
        assert_eq!(settled.fresh_input_tokens, 800);
        assert_eq!(settled.output_tokens, 8);
        assert_eq!(settled.attempts, 1);
        let remaining = remaining(&mut kernel);
        assert_eq!(remaining.output_tokens, 992);
        assert_eq!(remaining.attempts, 3);
        let _ = fs::remove_file(path);
    }
}

mod reported_usage_budget_release {
    use super::*;

    #[test]
    fn unused_output_reserve_is_released_between_model_turns() {
        let path = temp_db("reported-usage-budget-release");
        let mut kernel = kernel(&path);
        setup_root_with_output(&mut kernel, 256);
        setup_routing(&mut kernel, true, true);

        for ordinal in 1..=3 {
            let mut request = request(1_000);
            request.attribution.attempt_id = format!("attempt-{ordinal}");
            request.cache_epoch = ordinal;
            invoke::<_, StepRunnerResponse>(
                &mut kernel,
                step_runner_service(),
                &StepRunnerCommand::Run { request },
            )
            .unwrap();
        }

        let remaining = remaining(&mut kernel);
        assert_eq!(remaining.output_tokens, 232);
        assert_eq!(remaining.attempts, 1);
        let _ = fs::remove_file(path);
    }
}

mod failed_dispatch {
    use super::*;

    #[test]
    fn provider_failure_after_preflight_settles_reserved_maximum() {
        let path = temp_db("dispatch-failure");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, true);
        let mut request = request(1_000);
        request.input = b"provider-fails".to_vec().into();
        let error = invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run { request },
        )
        .unwrap_err();
        assert!(error.contains("fixture provider failed after invocation"));
        let attempt = lookup_attempt(&mut kernel, "attempt-1").expect("failed attempt exists");
        assert_eq!(attempt.phase, StepAttemptPhase::Settled);
        assert_eq!(attempt.outcome, Some(AttemptOutcome::Failed));
        let remaining = remaining(&mut kernel);
        assert_eq!(remaining.fresh_input_tokens, 3_200);
        assert_eq!(remaining.output_tokens, 872);
        assert_eq!(remaining.cost_microunits, Some(9_000));
        assert_eq!(remaining.attempts, 3);
        let _ = fs::remove_file(path);
    }
}

mod automatic_dispatch_retry {
    use super::*;

    #[test]
    fn retryable_provider_failure_uses_a_real_retry_attempt_and_next_candidate() {
        let path = temp_db("automatic-dispatch-retry");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, true);

        let mut request = request(1_000);
        request.input = b"provider-unavailable".to_vec().into();
        request.task.context.mandatory_input_tokens = 100;
        request.task.context.reducible_input_tokens = 100;
        request.context_candidates[0].estimated_tokens = 100;

        let response: StepRunnerResponse = invoke(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run { request },
        )
        .unwrap();
        let StepRunnerResponse::Completed {
            attempt, output, ..
        } = response;

        assert_eq!(attempt.attribution.kind, UsageAttemptKind::Retry);
        assert_eq!(
            attempt.attribution.parent_attempt_id.as_deref(),
            Some("attempt-1")
        );
        assert_eq!(
            attempt.route.as_ref().unwrap().target.model.as_str(),
            "large"
        );
        assert_eq!(output.as_ref(), b"provider-unavailable");

        let first = lookup_attempt(&mut kernel, "attempt-1").expect("root attempt exists");
        assert_eq!(first.phase, StepAttemptPhase::Settled);
        assert_eq!(first.outcome, Some(AttemptOutcome::Failed));
        assert_eq!(first.route.as_ref().unwrap().target.model.as_str(), "small");
        assert_eq!(remaining(&mut kernel).attempts, 2);
        let _ = fs::remove_file(path);
    }
}

mod retry_budget {
    use super::*;

    #[test]
    fn retry_limit_is_lineage_bound_not_reset_per_retry() {
        let path = temp_db("retry-budget");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, false);

        assert!(invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: request(1_000),
            },
        )
        .is_err());
        assert!(invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: retry_request("attempt-2", "attempt-1", 2),
            },
        )
        .is_err());

        let error = invoke::<_, StepRunnerResponse>(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: retry_request("attempt-3", "attempt-2", 3),
            },
        )
        .unwrap_err();
        assert!(error.contains("exceeds attempt limit"));
        assert!(lookup_attempt(&mut kernel, "attempt-3").is_none());
        assert_eq!(remaining(&mut kernel).attempts, 4);
        let _ = fs::remove_file(path);
    }
}

mod delegated_worker_runtime {
    use super::*;

    #[test]
    fn admitted_delegated_task_runs_on_pinned_route_and_reenters_parent_context() {
        let path = temp_db("delegated-worker-runtime");
        let mut kernel = kernel(&path);
        setup_root(&mut kernel);
        setup_routing(&mut kernel, true, true);

        let mut root_request = request(3_000);
        root_request.attribution.attempt_id = "root-attempt".into();
        root_request.task.context.mandatory_input_tokens = 2_000;
        root_request.task.context.reducible_input_tokens = 0;
        let root_response: StepRunnerResponse = invoke(
            &mut kernel,
            step_runner_service(),
            &StepRunnerCommand::Run {
                request: root_request,
            },
        )
        .unwrap();
        let StepRunnerResponse::Completed {
            attempt: root_attempt,
            ..
        } = root_response;
        let parent_plan = root_attempt.plan.clone();
        let route = root_attempt.route.clone().expect("root route is pinned");

        let root_execution: ExecutionResponse = invoke(
            &mut kernel,
            execution_service(),
            &ExecutionCommand::GetExecution { id: "root".into() },
        )
        .unwrap();
        let ExecutionResponse::ExecutionLookup {
            execution: Some(root_execution),
        } = root_execution
        else {
            panic!("root execution must exist");
        };

        let budget = BudgetReservation {
            input_tokens: 1_000,
            output_tokens: 128,
            cost_microunits: Some(1_000),
        };
        let authority = ExecutionAuthority::new(Vec::<String>::new());
        let contract: phenix_core::Bytes = b"inspect delegated subsystem".to_vec().into();
        let binding = DelegationTaskBinding {
            contract_revision: ArtifactRevision::from_content(contract.as_slice()),
            contract,
            parent_policy_revision: parent_plan.policy_revision.clone(),
            parent_plan: Some(parent_plan),
            originating_attempt_id: Some(root_attempt.attribution.attempt_id.clone()),
            resources: DelegatedWorkResources {
                target: route,
                authority: authority.clone(),
                context: Vec::new(),
                budget: budget.clone(),
                deadline_at_ms: 10_000,
                depth: 1,
                attempts: 1,
                max_result_bytes: 64 * 1024,
            },
        };
        let task = WorkerTaskRecord {
            id: "delegated-task-1".into(),
            parent_execution: "root".into(),
            graph_generation: root_execution.graph_generation,
            description: "inspect delegated subsystem".into(),
            depends_on: BTreeSet::new(),
            delegated_authority: authority.clone(),
            state: WorkerTaskState::Pending,
        };
        let policy = DelegationResourcePolicy {
            enabled: true,
            max_depth: 2,
            max_children: 2,
            max_attempts: 2,
            max_result_bytes: 64 * 1024,
        };
        let admitted: ExecutionResourceResponse = invoke(
            &mut kernel,
            execution_resource_service(),
            &ExecutionResourceCommand::AdmitDelegated {
                root_execution_id: "root".into(),
                reservation: BudgetReservationRequest {
                    reservation_id: "delegation/delegated-task-1".into(),
                    parent_reservation_id: None,
                    policy_revision: "policy-1".into(),
                    purpose: BudgetReservationPurpose::Delegation,
                    budget,
                    attempts: 1,
                },
                task,
                binding,
                parent_authority: root_execution.authority,
                policy,
                now_ms: 2_000,
            },
        )
        .unwrap();
        assert!(matches!(
            admitted,
            ExecutionResourceResponse::DelegatedTask { .. }
        ));

        let worked: DelegatedWorkerResponse = invoke(
            &mut kernel,
            delegated_worker_service(),
            &DelegatedWorkerCommand::RunNext { now_ms: 2_000 },
        )
        .unwrap();
        let DelegatedWorkerResponse::Processed {
            task,
            parent_admitted,
        } = worked
        else {
            panic!("expected one delegated task to run");
        };
        assert!(matches!(task.task.state, WorkerTaskState::Completed { .. }));
        assert!(parent_admitted);
        let result = task.result.expect("completed delegated task has a result");
        assert_eq!(result.findings.len(), 1);
        assert!(result.findings[0]
            .summary
            .contains("inspect delegated subsystem"));

        let attempts: StepAttemptResponse = invoke(
            &mut kernel,
            step_attempt_service(),
            &StepAttemptCommand::ListRoot {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let StepAttemptResponse::Attempts { attempts } = attempts else {
            panic!("expected root attempt list");
        };
        let delegated_attempt = attempts
            .iter()
            .find(|attempt| {
                attempt.attribution.kind == UsageAttemptKind::Delegated
                    && attempt.attribution.task_id.as_deref() == Some("delegated-task-1")
                    && attempt.attribution.parent_attempt_id.as_deref() == Some("root-attempt")
                    && attempt.outcome == Some(AttemptOutcome::Succeeded)
            })
            .expect("delegated attempt must be durably attributed");
        let root_attempt = attempts
            .iter()
            .find(|attempt| attempt.attribution.attempt_id == "root-attempt")
            .expect("originating attempt must remain available");
        assert_eq!(root_attempt.reacquisition.len(), 1);
        let reacquisition = &root_attempt.reacquisition[0];
        assert_eq!(
            reacquisition.reacquisition_id,
            "delegation:delegated-task-1:parent-context"
        );
        assert_eq!(reacquisition.cause_identity, "delegation:delegated-task-1");
        assert_eq!(
            reacquisition.source_attempt_id.as_deref(),
            Some(delegated_attempt.attribution.attempt_id.as_str())
        );
        assert!(matches!(
            &reacquisition.fresh_input_tokens,
            phenix_core::UsageQuantity::Estimated { value, .. } if *value > 0
        ));

        let replayed: DelegatedWorkerResponse = invoke(
            &mut kernel,
            delegated_worker_service(),
            &DelegatedWorkerCommand::RunTask {
                task_id: "delegated-task-1".into(),
                now_ms: 2_001,
            },
        )
        .unwrap();
        let DelegatedWorkerResponse::Processed {
            parent_admitted, ..
        } = replayed
        else {
            panic!("expected completed delegated task replay");
        };
        assert!(parent_admitted);

        let attempts: StepAttemptResponse = invoke(
            &mut kernel,
            step_attempt_service(),
            &StepAttemptCommand::ListRoot {
                root_execution_id: "root".into(),
            },
        )
        .unwrap();
        let StepAttemptResponse::Attempts { attempts } = attempts else {
            panic!("expected root attempt list after replay");
        };
        let root_attempt = attempts
            .iter()
            .find(|attempt| attempt.attribution.attempt_id == "root-attempt")
            .expect("originating attempt must remain available");
        assert_eq!(root_attempt.reacquisition.len(), 1);

        let idle: DelegatedWorkerResponse = invoke(
            &mut kernel,
            delegated_worker_service(),
            &DelegatedWorkerCommand::RunNext { now_ms: 2_001 },
        )
        .unwrap();
        assert_eq!(idle, DelegatedWorkerResponse::Idle);
        let _ = fs::remove_file(path);
    }
}
