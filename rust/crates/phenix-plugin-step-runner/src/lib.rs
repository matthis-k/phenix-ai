#![forbid(unsafe_code)]

use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface, ComponentManifest,
    ModelToolDescriptor, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ServiceContribution, ServiceId, SdkClient,
};
use phenix_sdk::{
    step_runner_service, AttemptOutcome, BudgetActual, BudgetReservationPurpose,
    BudgetReservationRequest, ContextAdmissionRequest, ContextCommand, ContextInterface,
    ContextResponse, ExecutionCommand, ExecutionInterface, ExecutionResourceCommand,
    ExecutionResourceInterface, ExecutionResourceResponse, ExecutionResponse, ExecutionState,
    ModelCommand, ModelDispatchCommand, ModelDispatchInterface, ModelDispatchResponse, ModelResponse,
    ModelRoutingInterface, PlannedStepRequest, StepAttemptCommand, StepAttemptInterface,
    StepAttemptRecord, StepAttemptResponse, StepPlan, StepRunnerCommand, StepRunnerInterface,
    StepRunnerResponse, StepSettlementBasis, UsageAttemptKind, UsageAttribution,
    UsagePlanningInput,
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
            import(ExecutionInterface::interface_id(), ExecutionInterface::schema()),
            import(
                ExecutionResourceInterface::interface_id(),
                ExecutionResourceInterface::schema(),
            ),
            import(
                StepAttemptInterface::interface_id(),
                StepAttemptInterface::schema(),
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
    routing: SdkClient<'host, 'runtime, ModelRoutingInterface>,
    dispatch: SdkClient<'host, 'runtime, ModelDispatchInterface>,
    context: SdkClient<'host, 'runtime, ContextInterface>,
}

type StepRunnerContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, StepRunnerSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> StepRunnerContext<'host, 'runtime> {
    let component = step_runner_component_id();
    PluginContext::new(
        host,
        StepRunnerSdk {
            execution: SdkClient::new(host, component.clone()),
            resources: SdkClient::new(host, component.clone()),
            attempts: SdkClient::new(host, component.clone()),
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

fn run(
    context: &StepRunnerContext<'_, '_>,
    request: PlannedStepRequest,
) -> Result<StepRunnerResponse, String> {
    let PlannedStepRequest {
        attribution,
        profile_id,
        callable_id,
        input,
        tools,
        policy,
        task,
        context_candidates,
        cache_epoch,
        route_policy,
        now_ms,
    } = request;

    if !matches!(attribution.kind, UsageAttemptKind::Root | UsageAttemptKind::Retry) {
        return Err("planned step runner accepts root and retry attempts only".into());
    }
    if attribution.policy_revision != policy.revision {
        return Err("planned step attribution policy revision does not match UsagePolicy".into());
    }

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
        return Err(format!(
            "planned step execution is not active: {}",
            attribution.execution_id
        ));
    }

    let remaining: ExecutionResourceResponse = context
        .sdk
        .resources
        .invoke_projected(&ExecutionResourceCommand::Remaining {
            root_execution_id: attribution.root_execution_id.clone(),
        })
        .map_err(|error| error.to_string())?;
    let ExecutionResourceResponse::Remaining { budget: remaining } = remaining else {
        return Err("execution resource service returned a non-remaining response".into());
    };
    let plan = policy
        .plan(&UsagePlanningInput {
            task,
            execution_state: execution.state,
            remaining,
            now_ms,
        })
        .map_err(|error| format!("usage planning failed: {error:?}"))?;
    validate_tools(&tools, &plan)?;
    validate_retry_lineage(context, &attribution, &plan)?;

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
    let purpose = match attribution.kind {
        UsageAttemptKind::Root => BudgetReservationPurpose::RootStep,
        UsageAttemptKind::Retry => BudgetReservationPurpose::Retry,
        _ => unreachable!("attempt kind checked above"),
    };
    let reserved: ExecutionResourceResponse = context
        .sdk
        .resources
        .invoke_projected(&ExecutionResourceCommand::Reserve {
            root_execution_id: attribution.root_execution_id.clone(),
            reservation: BudgetReservationRequest {
                reservation_id: reservation_id.clone(),
                parent_reservation_id: None,
                policy_revision: plan.policy_revision.clone(),
                purpose,
                budget: plan.reservation.clone(),
                attempts: 1,
            },
        })
        .map_err(|error| error.to_string())?;
    if !matches!(reserved, ExecutionResourceResponse::RootBudget { .. }) {
        return Err("execution resource service returned a non-budget response to reserve".into());
    }
    bind_attempt(
        context,
        StepAttemptCommand::BindReservation {
            attempt_id: attribution.attempt_id.clone(),
            reservation_id: reservation_id.clone(),
        },
    )?;

    let routed: ModelResponse = context
        .sdk
        .routing
        .invoke_projected(&ModelCommand::ResolveWithRequirements {
            profile_id,
            callable_id,
            requirements: plan.routing.clone(),
            policy: route_policy,
        })
        .map_err(|error| error.to_string())?;
    let ModelResponse::Decision { selection } = routed else {
        return Err("model routing returned a non-decision response".into());
    };
    let decision = selection.decision;
    bind_attempt(
        context,
        StepAttemptCommand::BindRoute {
            attempt_id: attribution.attempt_id.clone(),
            decision: decision.clone(),
        },
    )?;

    let admitted: ContextResponse = context
        .sdk
        .context
        .invoke_projected(&ContextCommand::Admit {
            request: ContextAdmissionRequest {
                execution_id: attribution.execution_id.clone(),
                step_plan: plan.clone(),
                candidates: context_candidates,
                cache_epoch,
            },
        })
        .map_err(|error| error.to_string())?;
    let ContextResponse::Admission { projection, .. } = admitted else {
        return Err("context service returned a non-admission response".into());
    };
    bind_attempt(
        context,
        StepAttemptCommand::BindProjection {
            attempt_id: attribution.attempt_id.clone(),
            projection,
        },
    )?;

    let dispatch_id = format!("dispatch/{}", attribution.attempt_id);
    bind_attempt(
        context,
        StepAttemptCommand::MarkDispatched {
            attempt_id: attribution.attempt_id.clone(),
            dispatch_id,
        },
    )?;

    let dispatched: ModelDispatchResponse = match context
        .sdk
        .dispatch
        .invoke_projected(&ModelDispatchCommand::InvokeResolved {
            decision,
            input,
            tools,
        })
    {
        Ok(response) => response,
        Err(error) => {
            settle_after_dispatch(
                context,
                &attribution.root_execution_id,
                &attribution.attempt_id,
                &plan,
                &reservation_id,
                AttemptOutcome::Failed,
            )?;
            return Err(format!("resolved model dispatch failed: {error}"));
        }
    };
    let ModelDispatchResponse::Inference { response, .. } = dispatched;

    let settled = conservative_actual(&plan);
    settle_budget(
        context,
        &attribution.root_execution_id,
        &reservation_id,
        settled.clone(),
    )?;
    let attempt = settle_attempt(context, &attribution.attempt_id, AttemptOutcome::Succeeded)?;

    Ok(StepRunnerResponse::Completed {
        attempt,
        output: response.output,
        tool_calls: response.tool_calls,
        settled,
        settlement_basis: StepSettlementBasis::ReservedMaximum,
    })
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

fn settle_attempt(
    context: &StepRunnerContext<'_, '_>,
    attempt_id: &str,
    outcome: AttemptOutcome,
) -> Result<StepAttemptRecord, String> {
    let response: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::Settle {
            attempt_id: attempt_id.to_owned(),
            outcome,
        })
        .map_err(|error| error.to_string())?;
    let StepAttemptResponse::Attempt { attempt } = response else {
        return Err("step attempt service returned a non-attempt settlement response".into());
    };
    Ok(attempt)
}

fn settle_budget(
    context: &StepRunnerContext<'_, '_>,
    root_execution_id: &str,
    reservation_id: &str,
    actual: BudgetActual,
) -> Result<(), String> {
    let response: ExecutionResourceResponse = context
        .sdk
        .resources
        .invoke_projected(&ExecutionResourceCommand::SettleReservation {
            root_execution_id: root_execution_id.to_owned(),
            reservation_id: reservation_id.to_owned(),
            actual,
        })
        .map_err(|error| error.to_string())?;
    if matches!(response, ExecutionResourceResponse::RootBudget { .. }) {
        Ok(())
    } else {
        Err("execution resource service returned a non-budget response to settle".into())
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
    settle_budget(
        context,
        root_execution_id,
        reservation_id,
        conservative_actual(plan),
    )?;
    settle_attempt(context, attempt_id, outcome).map(|_| ())
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
        assert_eq!(component.imports.len(), 6);
        assert!(component.imports.iter().all(|import| import.required));
        assert_eq!(component.exports.len(), 1);
        assert_eq!(component.exports[0].interface, StepRunnerInterface::interface_id());
    }
}
