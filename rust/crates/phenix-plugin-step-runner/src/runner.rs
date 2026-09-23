#![forbid(unsafe_code)]

use phenix_core::{
    Authority, CallError, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, ModelToolDescriptor, PluginContext, PluginExecution, PluginHost, PluginId,
    PluginInstance, PluginManifest, RuntimeTraceEvent, SdkClient, ServiceContribution, ServiceId,
};
use phenix_sdk::{
    select_route, step_runner_service, AttemptOutcome, BudgetActual, BudgetReservationPurpose,
    BudgetReservationRequest, ContextAdmissionRequest, ContextCommand, ContextInterface,
    ContextResponse, ExecutionCommand, ExecutionInterface, ExecutionResourceCommand,
    ExecutionResourceInterface, ExecutionResourceResponse, ExecutionResponse, ExecutionState,
    ModelCommand, ModelDispatchCommand, ModelDispatchFailure, ModelDispatchInterface,
    ModelDispatchResponse, ModelResponse, ModelRoutingInterface, PlannedStepRequest,
    ProjectionRevision, RouteSelection, StepAttemptCommand, StepAttemptInterface,
    StepAttemptRecord, StepAttemptResponse, StepPlan, StepRunnerCommand, StepRunnerInterface,
    StepRunnerResponse, StepSettlementBasis, StepTransactionCommand, StepTransactionInterface,
    StepTransactionResponse, UsageAttemptKind, UsageAttribution, UsagePlanningInput,
};
use std::collections::{BTreeMap, BTreeSet};

pub const STEP_RUNNER_PLUGIN: &str = "phenix.step-runner";
pub const STEP_RUNNER_COMPONENT: &str = "phenix.step-runner";

#[must_use]
pub fn step_runner_manifest(maximum_authority: Authority) -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(STEP_RUNNER_PLUGIN).expect("static step runner plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: step_runner_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: Vec::new(),
        maximum_authority,
    }
}

#[must_use]
pub fn step_runner_component_id() -> ComponentId {
    ComponentId::parse(STEP_RUNNER_COMPONENT).expect("static step runner component id is valid")
}

#[must_use]
pub fn step_runner_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let import = |interface, schema| ComponentImport {
        interface,
        schema,
        required: true,
        authority: maximum_authority.clone(),
    };
    ComponentManifest {
        listeners: Vec::new(),
        id: step_runner_component_id(),
        owner: PluginId::parse(STEP_RUNNER_PLUGIN).expect("static step runner plugin id is valid"),
        imports: vec![
            import(
                ExecutionInterface::interface_id(),
                ExecutionInterface::schema(),
            ),
            import(
                ExecutionResourceInterface::interface_id(),
                ExecutionResourceInterface::schema(),
            ),
            import(
                StepAttemptInterface::interface_id(),
                StepAttemptInterface::schema(),
            ),
            import(
                StepTransactionInterface::interface_id(),
                StepTransactionInterface::schema(),
            ),
            import(
                ModelRoutingInterface::interface_id(),
                ModelRoutingInterface::schema(),
            ),
            import(
                ModelDispatchInterface::interface_id(),
                ModelDispatchInterface::schema(),
            ),
            import(ContextInterface::interface_id(), ContextInterface::schema()),
        ],
        exports: vec![ComponentExport {
            interface: StepRunnerInterface::interface_id(),
            schema: StepRunnerInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority,
    }
}

#[must_use]
pub fn step_runner_factory() -> Box<dyn PluginInstance> {
    Box::new(StepRunnerPlugin)
}

struct StepRunnerSdk<'host, 'runtime> {
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
    resources: SdkClient<'host, 'runtime, ExecutionResourceInterface>,
    attempts: SdkClient<'host, 'runtime, StepAttemptInterface>,
    transactions: SdkClient<'host, 'runtime, StepTransactionInterface>,
    routing: SdkClient<'host, 'runtime, ModelRoutingInterface>,
    dispatch: SdkClient<'host, 'runtime, ModelDispatchInterface>,
    context: SdkClient<'host, 'runtime, ContextInterface>,
}

type StepRunnerContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, StepRunnerSdk<'host, 'runtime>>;

fn trace_policy_stage(
    context: &StepRunnerContext<'_, '_>,
    stage: &str,
    outcome: &str,
    revision: Option<&str>,
    reason: Option<String>,
) {
    context
        .kernel
        .record_runtime_trace(RuntimeTraceEvent::PolicyStage {
            policy: "phenix.step-runner".into(),
            stage: stage.into(),
            outcome: outcome.into(),
            subject: Some("planned_step".into()),
            revision: revision.map(str::to_owned),
            reason,
        });
}

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> StepRunnerContext<'host, 'runtime> {
    let component = step_runner_component_id();
    PluginContext::new(
        host,
        StepRunnerSdk {
            execution: SdkClient::new(host, component.clone()),
            resources: SdkClient::new(host, component.clone()),
            attempts: SdkClient::new(host, component.clone()),
            transactions: SdkClient::new(host, component.clone()),
            routing: SdkClient::new(host, component.clone()),
            dispatch: SdkClient::new(host, component.clone()),
            context: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

struct StepRunnerPlugin;

impl PluginInstance for StepRunnerPlugin {
    fn start(&mut self, _host: &PluginHost<'_>) -> Result<(), String> {
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &step_runner_service() {
            return Err(format!("unsupported step runner service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<StepRunnerCommand>(&StepRunnerInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let response = match command {
            StepRunnerCommand::Run { request } => run(&context, request)?,
        };
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetryRouteStrategy {
    PreferFallback,
    PreserveParent,
}

fn run(
    context: &StepRunnerContext<'_, '_>,
    request: PlannedStepRequest,
) -> Result<StepRunnerResponse, String> {
    run_with_retry_route(context, request, RetryRouteStrategy::PreferFallback)
}

fn run_with_retry_route(
    context: &StepRunnerContext<'_, '_>,
    request: PlannedStepRequest,
    retry_route_strategy: RetryRouteStrategy,
) -> Result<StepRunnerResponse, String> {
    let retry_template = request.clone();
    let PlannedStepRequest {
        attribution,
        profile_id,
        callable_id,
        input,
        tools,
        continuation,
        policy,
        task,
        context_candidates,
        cache_epoch,
        route_policy,
        now_ms,
    } = request;

    if !is_supported_attempt_kind(attribution.kind) {
        let reason = format!(
            "planned step runner does not support {:?} attempts",
            attribution.kind
        );
        trace_policy_stage(
            context,
            "attempt_kind",
            "denied",
            Some(&policy.revision),
            Some(reason.clone()),
        );
        return Err(reason);
    }
    trace_policy_stage(
        context,
        "attempt_kind",
        "allowed",
        Some(&policy.revision),
        None,
    );
    if attribution.policy_revision != policy.revision {
        let reason =
            "planned step attribution policy revision does not match UsagePolicy".to_owned();
        trace_policy_stage(
            context,
            "policy_revision",
            "denied",
            Some(&policy.revision),
            Some(reason.clone()),
        );
        return Err(reason);
    }
    trace_policy_stage(
        context,
        "policy_revision",
        "allowed",
        Some(&policy.revision),
        None,
    );

    let execution: ExecutionResponse = context
        .sdk
        .execution
        .invoke_projected(&ExecutionCommand::GetExecution {
            id: attribution.execution_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    let ExecutionResponse::ExecutionLookup {
        execution: Some(execution),
    } = execution
    else {
        return Err(format!(
            "unknown planned step execution: {}",
            attribution.execution_id
        ));
    };
    if execution.state != ExecutionState::Active {
        let reason = format!(
            "planned step execution is not active: {}",
            attribution.execution_id
        );
        trace_policy_stage(
            context,
            "execution_state",
            "denied",
            Some(&policy.revision),
            Some(reason.clone()),
        );
        return Err(reason);
    }
    trace_policy_stage(
        context,
        "execution_state",
        "allowed",
        Some(&policy.revision),
        None,
    );

    let parent_reservation_id = helper_parent_reservation(context, &attribution)?;
    let remaining_command = match &parent_reservation_id {
        Some(reservation_id) => ExecutionResourceCommand::RemainingWithin {
            root_execution_id: attribution.root_execution_id.clone(),
            reservation_id: reservation_id.clone(),
        },
        None => ExecutionResourceCommand::Remaining {
            root_execution_id: attribution.root_execution_id.clone(),
        },
    };
    let remaining: ExecutionResourceResponse = context
        .sdk
        .resources
        .invoke_projected(&remaining_command)
        .map_err(|error| error.to_string())?;
    let ExecutionResourceResponse::Remaining { budget: remaining } = remaining else {
        return Err("execution resource service returned a non-remaining response".into());
    };
    let plan = match policy.plan(&UsagePlanningInput {
        task,
        execution_state: execution.state,
        remaining,
        now_ms,
    }) {
        Ok(plan) => {
            trace_policy_stage(
                context,
                "usage_plan",
                "allowed",
                Some(&policy.revision),
                None,
            );
            plan
        }
        Err(error) => {
            let reason = format!("usage planning failed: {error:?}");
            trace_policy_stage(
                context,
                "usage_plan",
                "denied",
                Some(&policy.revision),
                Some(reason.clone()),
            );
            return Err(reason);
        }
    };
    if let Err(error) = validate_tools(&tools, &plan) {
        trace_policy_stage(
            context,
            "tool_set",
            "denied",
            Some(&plan.policy_revision),
            Some(error.clone()),
        );
        return Err(error);
    }
    trace_policy_stage(
        context,
        "tool_set",
        "allowed",
        Some(&plan.policy_revision),
        None,
    );
    if let Err(error) = validate_retry_lineage(context, &attribution, &plan) {
        trace_policy_stage(
            context,
            "retry_lineage",
            "denied",
            Some(&plan.policy_revision),
            Some(error.clone()),
        );
        return Err(error);
    }
    trace_policy_stage(
        context,
        "retry_lineage",
        "allowed",
        Some(&plan.policy_revision),
        None,
    );

    let created: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::Create {
            attribution: attribution.clone(),
            plan: plan.clone(),
        })
        .map_err(|error| error.to_string())?;
    let StepAttemptResponse::Attempt { .. } = created else {
        return Err("step attempt service returned a non-attempt response to create".into());
    };

    let reservation_id = format!("attempt/{}", attribution.attempt_id);
    let purpose = reservation_purpose(attribution.kind)?;
    let reserved: ExecutionResourceResponse =
        match context
            .sdk
            .resources
            .invoke_projected(&ExecutionResourceCommand::Reserve {
                root_execution_id: attribution.root_execution_id.clone(),
                reservation: BudgetReservationRequest {
                    reservation_id: reservation_id.clone(),
                    parent_reservation_id,
                    policy_revision: plan.policy_revision.clone(),
                    purpose,
                    budget: plan.reservation.clone(),
                    attempts: 1,
                },
            }) {
            Ok(response) => response,
            Err(error) => {
                let reason = format!("budget reservation failed: {error}");
                trace_policy_stage(
                    context,
                    "budget_reservation",
                    "denied",
                    Some(&plan.policy_revision),
                    Some(reason.clone()),
                );
                return fail_before_dispatch(
                    context,
                    &attribution.root_execution_id,
                    &attribution.attempt_id,
                    None,
                    reason,
                );
            }
        };
    if !matches!(reserved, ExecutionResourceResponse::RootBudget { .. }) {
        let reason =
            "execution resource service returned a non-budget response to reserve".to_owned();
        trace_policy_stage(
            context,
            "budget_reservation",
            "denied",
            Some(&plan.policy_revision),
            Some(reason.clone()),
        );
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            reason,
        );
    }
    trace_policy_stage(
        context,
        "budget_reservation",
        "allowed",
        Some(&plan.policy_revision),
        None,
    );
    if let Err(error) = bind_attempt(
        context,
        StepAttemptCommand::BindReservation {
            attempt_id: attribution.attempt_id.clone(),
            reservation_id: reservation_id.clone(),
        },
    ) {
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            error,
        );
    }

    let routed: ModelResponse = match resolve_model_route(
        context,
        &attribution,
        profile_id,
        callable_id,
        &plan,
        route_policy,
        retry_route_strategy,
    ) {
        Ok(response) => response,
        Err(error) => {
            let reason = format!("model routing failed: {error}");
            trace_policy_stage(
                context,
                "model_routing",
                "denied",
                Some(&plan.policy_revision),
                Some(reason.clone()),
            );
            return fail_before_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                Some(&reservation_id),
                reason,
            );
        }
    };
    let ModelResponse::Decision { selection } = routed else {
        let reason = "model routing returned a non-decision response".to_owned();
        trace_policy_stage(
            context,
            "model_routing",
            "denied",
            Some(&plan.policy_revision),
            Some(reason.clone()),
        );
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            reason,
        );
    };
    trace_policy_stage(
        context,
        "model_routing",
        "allowed",
        Some(&plan.policy_revision),
        None,
    );
    let decision = selection.decision;
    if let Err(error) = bind_attempt(
        context,
        StepAttemptCommand::BindRoute {
            attempt_id: attribution.attempt_id.clone(),
            decision: decision.clone(),
        },
    ) {
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            error,
        );
    }

    let model_input = if is_helper_attempt(attribution.kind) {
        let projection = ProjectionRevision {
            revision: 0,
            cache_epoch: 0,
        };
        if let Err(error) = bind_attempt(
            context,
            StepAttemptCommand::BindProjection {
                attempt_id: attribution.attempt_id.clone(),
                projection,
            },
        ) {
            return fail_before_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                Some(&reservation_id),
                error,
            );
        }
        input
    } else {
        let admitted: ContextResponse =
            match context
                .sdk
                .context
                .invoke_projected(&ContextCommand::Admit {
                    request: ContextAdmissionRequest {
                        execution_id: attribution.execution_id.clone(),
                        step_plan: plan.clone(),
                        candidates: context_candidates,
                        cache_epoch,
                    },
                }) {
                Ok(response) => response,
                Err(error) => {
                    let reason = format!("context admission failed: {error}");
                    trace_policy_stage(
                        context,
                        "context_admission",
                        "denied",
                        Some(&plan.policy_revision),
                        Some(reason.clone()),
                    );
                    return fail_before_dispatch(
                        context,
                        &attribution.root_execution_id,
                        &attribution.attempt_id,
                        Some(&reservation_id),
                        reason,
                    );
                }
            };
        let ContextResponse::Admission { projection, .. } = admitted else {
            let reason = "context service returned a non-admission response".to_owned();
            trace_policy_stage(
                context,
                "context_admission",
                "denied",
                Some(&plan.policy_revision),
                Some(reason.clone()),
            );
            return fail_before_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                Some(&reservation_id),
                reason,
            );
        };
        trace_policy_stage(
            context,
            "context_admission",
            "allowed",
            Some(&plan.policy_revision),
            None,
        );
        if let Err(error) = bind_attempt(
            context,
            StepAttemptCommand::BindProjection {
                attempt_id: attribution.attempt_id.clone(),
                projection: projection.clone(),
            },
        ) {
            return fail_before_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                Some(&reservation_id),
                error,
            );
        }

        let materialized: ContextResponse =
            match context
                .sdk
                .context
                .invoke_projected(&ContextCommand::MaterializeInvocation {
                    execution_id: attribution.execution_id.clone(),
                    input,
                    expected_projection: projection.clone(),
                }) {
                Ok(response) => response,
                Err(error) => {
                    return fail_before_dispatch(
                        context,
                        &attribution.root_execution_id,
                        &attribution.attempt_id,
                        Some(&reservation_id),
                        format!("context materialization failed: {error}"),
                    )
                }
            };
        let ContextResponse::InvocationMaterialized { materialization } = materialized else {
            return fail_before_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                Some(&reservation_id),
                "context service returned a non-materialization response".into(),
            );
        };
        if materialization.projection != projection {
            return fail_before_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                Some(&reservation_id),
                "context materialization changed the admitted projection".into(),
            );
        }
        materialization.input
    };

    let prepared: ModelDispatchResponse =
        match context
            .sdk
            .dispatch
            .invoke_fallible_projected::<
                ModelDispatchCommand,
                ModelDispatchResponse,
                ModelDispatchFailure,
            >(&ModelDispatchCommand::PrepareResolved {
                decision: decision.clone(),
                input: model_input,
                tools,
                continuation,
            }) {
            Ok(response) => response,
            Err(error) => {
                let reason = match error {
                    CallError::Domain(failure) => format!(
                        "resolved model preflight failed: {}",
                        failure.failure.message()
                    ),
                    CallError::Runtime(error) => {
                        format!("resolved model preflight runtime failure: {error}")
                    }
                    CallError::Conversion(error) => {
                        format!("resolved model preflight conversion failure: {error}")
                    }
                };
                trace_policy_stage(
                    context,
                    "dispatch_preflight",
                    "denied",
                    Some(&plan.policy_revision),
                    Some(reason.clone()),
                );
                return fail_before_dispatch(
                    context,
                    &attribution.root_execution_id,
                    &attribution.attempt_id,
                    Some(&reservation_id),
                    reason,
                );
            }
        };
    let ModelDispatchResponse::Ready { prepared } = prepared else {
        let reason = "model dispatch returned inference during preflight".to_owned();
        trace_policy_stage(
            context,
            "dispatch_preflight",
            "denied",
            Some(&plan.policy_revision),
            Some(reason.clone()),
        );
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            reason,
        );
    };
    trace_policy_stage(
        context,
        "dispatch_preflight",
        "allowed",
        Some(&plan.policy_revision),
        None,
    );
    if prepared.decision() != &decision {
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            "model dispatch preflight changed the resolved decision".into(),
        );
    }

    let dispatch_id = format!("dispatch/{}", attribution.attempt_id);
    if let Err(error) = bind_attempt(
        context,
        StepAttemptCommand::MarkDispatched {
            attempt_id: attribution.attempt_id.clone(),
            dispatch_id,
        },
    ) {
        return fail_before_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            Some(&reservation_id),
            error,
        );
    }

    let dispatched: ModelDispatchResponse = match context
        .sdk
        .dispatch
        .invoke_fallible_projected::<
            ModelDispatchCommand,
            ModelDispatchResponse,
            ModelDispatchFailure,
        >(&ModelDispatchCommand::InvokePrepared { prepared })
    {
        Ok(response) => response,
        Err(CallError::Domain(failure)) => {
            settle_after_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                &plan,
                &reservation_id,
                AttemptOutcome::Failed,
            )?;
            if failure.requires_context_recovery()
                && retry_template.task.context.reducible_input_tokens > 0
                && retry_available(context, &attribution, &plan)?
            {
                let mut recovery_request = retry_template;
                recovery_request.task.context.reducible_input_tokens = 0;
                trace_policy_stage(
                    context,
                    "context_overflow_recovery",
                    "allowed",
                    Some(&plan.policy_revision),
                    Some(format!(
                        "retrying candidate {} with reducible context pruned",
                        decision.candidate_ordinal
                    )),
                );
                return retry_step(
                    context,
                    recovery_request,
                    &attribution,
                    RetryRouteStrategy::PreserveParent,
                );
            }
            if failure.retryable() && retry_available(context, &attribution, &plan)? {
                trace_policy_stage(
                    context,
                    "dispatch_retry",
                    "allowed",
                    Some(&plan.policy_revision),
                    Some(format!(
                        "retrying after {:?} on candidate {}",
                        failure.failure, decision.candidate_ordinal
                    )),
                );
                return retry_step(
                    context,
                    retry_template,
                    &attribution,
                    RetryRouteStrategy::PreferFallback,
                );
            }
            return Err(format!(
                "prepared model dispatch failed: {}",
                failure.failure.message()
            ));
        }
        Err(CallError::Runtime(error)) => {
            settle_after_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                &plan,
                &reservation_id,
                AttemptOutcome::Failed,
            )?;
            return Err(format!("prepared model dispatch runtime failure: {error}"));
        }
        Err(CallError::Conversion(error)) => {
            settle_after_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                &plan,
                &reservation_id,
                AttemptOutcome::Failed,
            )?;
            return Err(format!("prepared model dispatch conversion failure: {error}"));
        }
    };
    let ModelDispatchResponse::Inference { response, .. } = dispatched else {
        settle_after_dispatch(
            context,
            &attribution.root_execution_id,
            &attribution.attempt_id,
            &plan,
            &reservation_id,
            AttemptOutcome::Failed,
        )?;
        return Err("model dispatch returned preflight readiness after dispatch".into());
    };

    let (settled, settlement_basis) = successful_actual(&plan, &response.usage);
    let attempt = settle_step(
        context,
        &attribution.root_execution_id,
        &reservation_id,
        settled.clone(),
        &attribution.attempt_id,
        AttemptOutcome::Succeeded,
    )?;

    Ok(StepRunnerResponse::Completed {
        attempt,
        output: response.output,
        tool_calls: response.tool_calls,
        settled,
        settlement_basis,
    })
}

fn resolve_model_route(
    context: &StepRunnerContext<'_, '_>,
    attribution: &UsageAttribution,
    profile_id: phenix_core::RoutingProfileId,
    callable_id: Option<phenix_core::CallableId>,
    plan: &StepPlan,
    route_policy: phenix_sdk::RouteSelectionPolicy,
    retry_route_strategy: RetryRouteStrategy,
) -> Result<ModelResponse, String> {
    if attribution.kind == UsageAttemptKind::Retry {
        let listed_attempts: StepAttemptResponse = context
            .sdk
            .attempts
            .invoke_projected(&StepAttemptCommand::ListRoot {
                root_execution_id: attribution.root_execution_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        let StepAttemptResponse::Attempts { attempts } = listed_attempts else {
            return Err("step attempt service returned a non-list response".into());
        };
        let attempts = attempts
            .into_iter()
            .map(|attempt| (attempt.attribution.attempt_id.clone(), attempt))
            .collect::<BTreeMap<_, _>>();

        let parent_attempt_id = attribution
            .parent_attempt_id
            .as_ref()
            .ok_or_else(|| "planned retry requires a parent attempt".to_owned())?;
        let parent = attempts
            .get(parent_attempt_id)
            .ok_or_else(|| format!("unknown planned retry parent: {parent_attempt_id}"))?;
        let parent_decision = parent
            .route
            .clone()
            .ok_or_else(|| "planned retry parent has no resolved route".to_owned())?;

        if retry_route_strategy == RetryRouteStrategy::PreserveParent {
            return Ok(ModelResponse::Decision {
                selection: RouteSelection {
                    decision: parent_decision,
                    rejected: Vec::new(),
                },
            });
        }

        let mut tried_ordinals = BTreeSet::new();
        let mut current = attribution.parent_attempt_id.clone();
        let mut seen = BTreeSet::new();
        while let Some(attempt_id) = current {
            if !seen.insert(attempt_id.clone()) {
                return Err("planned retry lineage contains a cycle".into());
            }
            let attempt = attempts
                .get(&attempt_id)
                .ok_or_else(|| format!("unknown planned retry ancestor: {attempt_id}"))?;
            if let Some(route) = &attempt.route {
                tried_ordinals.insert(route.candidate_ordinal);
            }
            current = attempt.attribution.parent_attempt_id.clone();
        }

        let listed: ModelResponse = context
            .sdk
            .routing
            .invoke_projected(&ModelCommand::ListCandidates {
                profile_id: profile_id.clone(),
                callable_id: callable_id.clone(),
            })
            .map_err(|error| error.to_string())?;
        let ModelResponse::Candidates { mut candidates } = listed else {
            return Err("model routing returned a non-candidate response".into());
        };
        candidates.retain(|candidate| !tried_ordinals.contains(&candidate.ordinal));
        if !candidates.is_empty() {
            if let Ok(selection) = select_route(&candidates, &plan.routing, &route_policy) {
                return Ok(ModelResponse::Decision { selection });
            }
        }
        return Ok(ModelResponse::Decision {
            selection: RouteSelection {
                decision: parent_decision,
                rejected: Vec::new(),
            },
        });
    }

    context
        .sdk
        .routing
        .invoke_projected(&ModelCommand::ResolveWithRequirements {
            profile_id,
            callable_id,
            requirements: plan.routing.clone(),
            policy: route_policy,
        })
        .map_err(|error| error.to_string())
}

fn retry_available(
    context: &StepRunnerContext<'_, '_>,
    attribution: &UsageAttribution,
    plan: &StepPlan,
) -> Result<bool, String> {
    if !matches!(
        attribution.kind,
        UsageAttemptKind::Root | UsageAttemptKind::Retry
    ) {
        return Ok(false);
    }
    let response: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::ListRoot {
            root_execution_id: attribution.root_execution_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    let StepAttemptResponse::Attempts { attempts } = response else {
        return Err("step attempt service returned a non-list response".into());
    };
    let attempts = attempts
        .into_iter()
        .map(|attempt| (attempt.attribution.attempt_id.clone(), attempt))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut current = Some(attribution.attempt_id.clone());
    let mut attempt_count = 0_u32;
    while let Some(attempt_id) = current {
        if !seen.insert(attempt_id.clone()) {
            return Err("planned retry lineage contains a cycle".into());
        }
        let attempt = attempts
            .get(&attempt_id)
            .ok_or_else(|| format!("unknown planned retry attempt: {attempt_id}"))?;
        if !matches!(
            attempt.attribution.kind,
            UsageAttemptKind::Root | UsageAttemptKind::Retry
        ) {
            return Ok(false);
        }
        attempt_count = attempt_count.saturating_add(1);
        current = attempt.attribution.parent_attempt_id.clone();
    }
    Ok(attempt_count < plan.retry.max_attempts)
}

fn retry_step(
    context: &StepRunnerContext<'_, '_>,
    mut request: PlannedStepRequest,
    parent: &UsageAttribution,
    retry_route_strategy: RetryRouteStrategy,
) -> Result<StepRunnerResponse, String> {
    let allocated: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::AllocateIdentity {
            root_execution_id: parent.root_execution_id.clone(),
            execution_id: parent.execution_id.clone(),
            parent_attempt_id: Some(parent.attempt_id.clone()),
            policy_revision: request.policy.revision.clone(),
            kind: UsageAttemptKind::Retry,
        })
        .map_err(|error| format!("retry attempt allocation failed: {error}"))?;
    let StepAttemptResponse::Attribution { attribution } = allocated else {
        return Err("step attempt service returned a non-attribution retry allocation".into());
    };
    request.attribution = attribution;
    run_with_retry_route(context, request, retry_route_strategy)
}

fn is_supported_attempt_kind(kind: UsageAttemptKind) -> bool {
    matches!(
        kind,
        UsageAttemptKind::Root
            | UsageAttemptKind::Retry
            | UsageAttemptKind::Helper
            | UsageAttemptKind::Verification
            | UsageAttemptKind::RecoveryClassifier
    )
}

fn is_helper_attempt(kind: UsageAttemptKind) -> bool {
    matches!(
        kind,
        UsageAttemptKind::Helper
            | UsageAttemptKind::Verification
            | UsageAttemptKind::RecoveryClassifier
    )
}

fn reservation_purpose(kind: UsageAttemptKind) -> Result<BudgetReservationPurpose, String> {
    match kind {
        UsageAttemptKind::Root => Ok(BudgetReservationPurpose::RootStep),
        UsageAttemptKind::Retry => Ok(BudgetReservationPurpose::Retry),
        UsageAttemptKind::Helper => Ok(BudgetReservationPurpose::Helper),
        UsageAttemptKind::Verification => Ok(BudgetReservationPurpose::Verification),
        UsageAttemptKind::RecoveryClassifier => Ok(BudgetReservationPurpose::RecoveryClassifier),
        _ => Err(format!("unsupported planned step attempt kind: {kind:?}")),
    }
}

fn helper_parent_reservation(
    context: &StepRunnerContext<'_, '_>,
    attribution: &UsageAttribution,
) -> Result<Option<String>, String> {
    if !is_helper_attempt(attribution.kind) {
        return Ok(None);
    }
    let parent_attempt_id = attribution
        .parent_attempt_id
        .as_ref()
        .ok_or_else(|| "helper attempt requires a parent attempt".to_owned())?;
    let response: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::Get {
            attempt_id: parent_attempt_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    let StepAttemptResponse::AttemptLookup {
        attempt: Some(parent),
    } = response
    else {
        return Err(format!(
            "unknown helper parent attempt: {parent_attempt_id}"
        ));
    };
    if parent.attribution.root_execution_id != attribution.root_execution_id {
        return Err("helper parent belongs to a different root execution".into());
    }
    parent
        .reservation_id
        .ok_or_else(|| "helper parent attempt has no active budget reservation".to_owned())
        .map(Some)
}

fn validate_tools(tools: &[ModelToolDescriptor], plan: &StepPlan) -> Result<(), String> {
    let provided = tools
        .iter()
        .map(|tool| tool.id.clone())
        .collect::<BTreeSet<_>>();
    if !plan.tools.initial.is_subset(&provided) {
        return Err("planned step is missing a required tool descriptor".into());
    }
    let allowed = plan
        .tools
        .initial
        .union(&plan.tools.expandable)
        .cloned()
        .collect::<BTreeSet<_>>();
    if !provided.is_subset(&allowed) {
        return Err("planned step provided a tool outside the planned tool set".into());
    }
    if provided.len() > plan.tools.max_schemas as usize {
        return Err("planned step tool descriptor count exceeds the plan".into());
    }
    Ok(())
}

fn validate_retry_lineage(
    context: &StepRunnerContext<'_, '_>,
    attribution: &UsageAttribution,
    plan: &StepPlan,
) -> Result<(), String> {
    if attribution.kind != UsageAttemptKind::Retry {
        return Ok(());
    }
    let parent = attribution
        .parent_attempt_id
        .clone()
        .ok_or_else(|| "planned retry requires a parent attempt".to_owned())?;
    let response: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::ListRoot {
            root_execution_id: attribution.root_execution_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    let StepAttemptResponse::Attempts { attempts } = response else {
        return Err("step attempt service returned a non-list response".into());
    };
    let attempts = attempts
        .into_iter()
        .map(|attempt| (attempt.attribution.attempt_id.clone(), attempt))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut current = Some(parent);
    let mut ancestor_count = 0_u32;
    while let Some(attempt_id) = current {
        if !seen.insert(attempt_id.clone()) {
            return Err("planned retry lineage contains a cycle".into());
        }
        let attempt = attempts
            .get(&attempt_id)
            .ok_or_else(|| format!("unknown planned retry parent: {attempt_id}"))?;
        if !matches!(
            attempt.attribution.kind,
            UsageAttemptKind::Root | UsageAttemptKind::Retry
        ) {
            return Err("planned retry parent is outside the root/retry lifecycle".into());
        }
        ancestor_count = ancestor_count.saturating_add(1);
        current = attempt.attribution.parent_attempt_id.clone();
    }
    let requested_attempt = ancestor_count.saturating_add(1);
    if requested_attempt > plan.retry.max_attempts {
        return Err(format!(
            "planned retry exceeds attempt limit: requested {requested_attempt}, allowed {}",
            plan.retry.max_attempts
        ));
    }
    Ok(())
}

fn bind_attempt(
    context: &StepRunnerContext<'_, '_>,
    command: StepAttemptCommand,
) -> Result<(), String> {
    let response: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&command)
        .map_err(|error| error.to_string())?;
    if matches!(response, StepAttemptResponse::Attempt { .. }) {
        Ok(())
    } else {
        Err("step attempt service returned a non-attempt mutation response".into())
    }
}

fn fail_before_dispatch<T>(
    context: &StepRunnerContext<'_, '_>,
    root_execution_id: &str,
    attempt_id: &str,
    reservation_id: Option<&str>,
    cause: String,
) -> Result<T, String> {
    match abort_before_dispatch(
        context,
        root_execution_id,
        attempt_id,
        reservation_id,
        AttemptOutcome::Failed,
    ) {
        Ok(_) => Err(cause),
        Err(cleanup) => Err(format!("{cause}; pre-dispatch cleanup failed: {cleanup}")),
    }
}

fn abort_before_dispatch(
    context: &StepRunnerContext<'_, '_>,
    root_execution_id: &str,
    attempt_id: &str,
    reservation_id: Option<&str>,
    outcome: AttemptOutcome,
) -> Result<StepAttemptRecord, String> {
    let response: StepTransactionResponse = context
        .sdk
        .transactions
        .invoke_projected(&StepTransactionCommand::AbortBeforeDispatch {
            root_execution_id: root_execution_id.to_owned(),
            reservation_id: reservation_id.map(str::to_owned),
            attempt_id: attempt_id.to_owned(),
            outcome,
        })
        .map_err(|error| error.to_string())?;
    match response {
        StepTransactionResponse::Aborted { attempt, .. } => Ok(attempt),
        StepTransactionResponse::Settled { .. } => {
            Err("step transaction service returned settlement to pre-dispatch abort".into())
        }
    }
}

fn settle_step(
    context: &StepRunnerContext<'_, '_>,
    root_execution_id: &str,
    reservation_id: &str,
    actual: BudgetActual,
    attempt_id: &str,
    outcome: AttemptOutcome,
) -> Result<StepAttemptRecord, String> {
    let response: StepTransactionResponse = context
        .sdk
        .transactions
        .invoke_projected(&StepTransactionCommand::Settle {
            root_execution_id: root_execution_id.to_owned(),
            reservation_id: reservation_id.to_owned(),
            actual,
            attempt_id: attempt_id.to_owned(),
            outcome,
        })
        .map_err(|error| error.to_string())?;
    match response {
        StepTransactionResponse::Settled { attempt, .. } => Ok(attempt),
        StepTransactionResponse::Aborted { .. } => {
            Err("step transaction service returned abort to terminal settlement".into())
        }
    }
}

fn settle_after_dispatch(
    context: &StepRunnerContext<'_, '_>,
    root_execution_id: &str,
    attempt_id: &str,
    plan: &StepPlan,
    reservation_id: &str,
    outcome: AttemptOutcome,
) -> Result<(), String> {
    settle_step(
        context,
        root_execution_id,
        reservation_id,
        conservative_actual(plan),
        attempt_id,
        outcome,
    )
    .map(|_| ())
}

fn successful_actual(
    plan: &StepPlan,
    usage: &phenix_core::ModelTurnUsage,
) -> (BudgetActual, StepSettlementBasis) {
    let output_tokens = match &usage.output_tokens {
        phenix_core::UsageQuantity::Reported { value } => *value,
        phenix_core::UsageQuantity::Estimated { .. } | phenix_core::UsageQuantity::Unavailable => {
            return (
                conservative_actual(plan),
                StepSettlementBasis::ReservedMaximum,
            );
        }
    };
    (
        BudgetActual {
            fresh_input_tokens: plan.reservation.input_tokens,
            output_tokens,
            cost_microunits: plan.reservation.cost_microunits,
            attempts: 1,
        },
        StepSettlementBasis::ProviderReportedOutput,
    )
}

fn conservative_actual(plan: &StepPlan) -> BudgetActual {
    BudgetActual {
        fresh_input_tokens: plan.reservation.input_tokens,
        output_tokens: plan.reservation.output_tokens,
        cost_microunits: plan.reservation.cost_microunits,
        attempts: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_uses_typed_imports_instead_of_plugin_dependencies() {
        assert!(step_runner_manifest(Authority::default())
            .dependencies
            .is_empty());
    }

    #[test]
    fn component_imports_each_state_owner_once() {
        let component = step_runner_component_manifest(Authority::default());
        assert_eq!(component.imports.len(), 7);
        assert!(component.imports.iter().all(|import| import.required));
        assert_eq!(component.exports.len(), 1);
        assert_eq!(
            component.exports[0].interface,
            StepRunnerInterface::interface_id()
        );
    }
}
