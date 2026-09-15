#![forbid(unsafe_code)]

mod runner;

use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentInvocationError, ComponentManifest, ContextResourceId, PhenixValue, PluginContext,
    PluginHost, PluginId, PluginInstance, PluginManifest, SdkClient, ServiceContribution,
    ServiceId, ServiceRole,
};
use phenix_sdk::{
    context_service, default_invocation_service, helper_invocation_service, invocation_service,
    step_runner_service, ContextAnchor, ContextCommand, ContextInjectionLifetime,
    ContextInjectionRequester, ContextInterface, ContextInvocationPreparation,
    ContextRecoveryCommand, ContextRecoveryDecision, ContextRecoveryInterface,
    ContextRecoveryRequest, ContextRecoveryResponse, ContextResourceKind, ContextResponse,
    ContextScope, DefaultInvocationCommand, DefaultInvocationInterface, ExecutionCommand,
    ExecutionInterface, ExecutionResponse, HelperInvocationCommand, HelperInvocationInterface,
    HelperInvocationResponse, InvocationClockCommand, InvocationClockInterface,
    InvocationClockResponse, InvocationCommand, InvocationDefaultsCommand,
    InvocationDefaultsInterface, InvocationDefaultsResponse, InvocationInterface, InvocationParams,
    InvocationRequest, MemoryCommand, MemoryContextCommand, MemoryContextInterface,
    MemoryContextRecallRequest, MemoryContextResponse, MemoryInterface, MemoryResponse,
    MemoryScope, PlannedStepRequest, ProjectionRevision, RecallEvidence, RecallResolution,
    StepAttemptCommand, StepAttemptInterface, StepAttemptResponse, StepRunnerCommand,
    StepRunnerResponse, UsageAttemptKind,
};
use std::collections::BTreeSet;

pub use runner::{step_runner_component_id, STEP_RUNNER_COMPONENT, STEP_RUNNER_PLUGIN};

pub const HELPER_INVOCATION_COMPONENT: &str = "phenix.helper-invocation";

#[must_use]
pub fn step_runner_manifest(maximum_authority: Authority) -> PluginManifest {
    let mut manifest = runner::step_runner_manifest(maximum_authority);
    for service in [
        invocation_service(),
        default_invocation_service(),
        helper_invocation_service(),
    ] {
        manifest.services.push(ServiceContribution {
            role: ServiceRole::Terminal,
            service,
            priority: 100,
            required_authority: Authority::default(),
        });
    }
    manifest
}

#[must_use]
pub fn step_runner_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let mut manifest = runner::step_runner_component_manifest(maximum_authority);
    let optional_import = |interface, schema| ComponentImport {
        interface,
        schema,
        required: false,
        authority: manifest.maximum_authority.clone(),
    };
    manifest.imports.push(optional_import(
        InvocationDefaultsInterface::interface_id(),
        InvocationDefaultsInterface::schema(),
    ));
    manifest.imports.push(ComponentImport {
        interface: InvocationClockInterface::interface_id(),
        schema: InvocationClockInterface::schema(),
        required: true,
        authority: manifest.maximum_authority.clone(),
    });
    manifest.imports.push(optional_import(
        ContextRecoveryInterface::interface_id(),
        ContextRecoveryInterface::schema(),
    ));
    manifest.imports.push(optional_import(
        MemoryContextInterface::interface_id(),
        MemoryContextInterface::schema(),
    ));
    manifest.imports.push(optional_import(
        MemoryInterface::interface_id(),
        MemoryInterface::schema(),
    ));
    for interface in [
        (
            InvocationInterface::interface_id(),
            InvocationInterface::schema(),
        ),
        (
            DefaultInvocationInterface::interface_id(),
            DefaultInvocationInterface::schema(),
        ),
    ] {
        manifest.exports.push(ComponentExport {
            interface: interface.0,
            schema: interface.1,
            priority: 100,
            required_authority: Authority::default(),
        });
    }
    manifest
}

#[must_use]
pub fn helper_invocation_component_id() -> ComponentId {
    ComponentId::parse(HELPER_INVOCATION_COMPONENT)
        .expect("static helper invocation component id is valid")
}

#[must_use]
pub fn helper_invocation_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let import = |interface, schema, required| ComponentImport {
        interface,
        schema,
        required,
        authority: maximum_authority.clone(),
    };
    ComponentManifest {
        listeners: Vec::new(),
        id: helper_invocation_component_id(),
        owner: PluginId::parse(STEP_RUNNER_PLUGIN).expect("static step runner plugin id is valid"),
        imports: vec![
            import(
                InvocationDefaultsInterface::interface_id(),
                InvocationDefaultsInterface::schema(),
                false,
            ),
            import(
                InvocationClockInterface::interface_id(),
                InvocationClockInterface::schema(),
                true,
            ),
            import(
                ExecutionInterface::interface_id(),
                ExecutionInterface::schema(),
                true,
            ),
            import(
                StepAttemptInterface::interface_id(),
                StepAttemptInterface::schema(),
                true,
            ),
        ],
        exports: vec![ComponentExport {
            interface: HelperInvocationInterface::interface_id(),
            schema: HelperInvocationInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority,
    }
}

#[must_use]
pub fn step_runner_factory() -> Box<dyn PluginInstance> {
    Box::new(InvocationPackage {
        runner: runner::step_runner_factory(),
    })
}

struct InvocationSdk<'host, 'runtime> {
    defaults: SdkClient<'host, 'runtime, InvocationDefaultsInterface>,
    clock: SdkClient<'host, 'runtime, InvocationClockInterface>,
    context: SdkClient<'host, 'runtime, ContextInterface>,
    recovery: SdkClient<'host, 'runtime, ContextRecoveryInterface>,
    memory_context: SdkClient<'host, 'runtime, MemoryContextInterface>,
    memory: SdkClient<'host, 'runtime, MemoryInterface>,
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
    attempts: SdkClient<'host, 'runtime, StepAttemptInterface>,
}

type InvocationContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, InvocationSdk<'host, 'runtime>>;

fn invocation_context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
    component: ComponentId,
) -> InvocationContext<'host, 'runtime> {
    PluginContext::new(
        host,
        InvocationSdk {
            defaults: SdkClient::new(host, component.clone()),
            clock: SdkClient::new(host, component.clone()),
            context: SdkClient::new(host, component.clone()),
            recovery: SdkClient::new(host, component.clone()),
            memory_context: SdkClient::new(host, component.clone()),
            memory: SdkClient::new(host, component.clone()),
            execution: SdkClient::new(host, component.clone()),
            attempts: SdkClient::new(host, component),
        },
        (),
        (),
    )
}

struct InvocationPackage {
    runner: Box<dyn PluginInstance>,
}

impl InvocationPackage {
    fn invoke_explicit(
        &mut self,
        context: &InvocationContext<'_, '_>,
        host: &PluginHost<'_>,
        request: InvocationRequest,
        params: InvocationParams,
    ) -> Result<Vec<u8>, String> {
        let kind = if request.parent_attempt_id.is_some() {
            UsageAttemptKind::Retry
        } else {
            UsageAttemptKind::Root
        };
        self.invoke_with_kind(context, host, request, params, kind)
    }

    fn invoke_with_kind(
        &mut self,
        context: &InvocationContext<'_, '_>,
        host: &PluginHost<'_>,
        request: InvocationRequest,
        params: InvocationParams,
        kind: UsageAttemptKind,
    ) -> Result<Vec<u8>, String> {
        let root_execution_id = root_execution_id(context, &request.execution_id)?;
        let mut preparation = if is_isolated_helper(kind) {
            ContextInvocationPreparation {
                request_input_tokens: u64::try_from(request.input.as_ref().len())
                    .unwrap_or(u64::MAX),
                candidates: Vec::new(),
                projection: ProjectionRevision {
                    revision: 0,
                    cache_epoch: 0,
                },
            }
        } else {
            prepare_invocation_context(context, &request)?
        };

        let clock: InvocationClockResponse = context
            .sdk
            .clock
            .invoke_projected(&InvocationClockCommand::Now)
            .map_err(|error| format!("invocation clock unavailable: {error}"))?;
        let InvocationClockResponse::Time { now_ms } = clock;

        if !is_isolated_helper(kind) {
            preparation = recover_invocation_context(
                context,
                &request,
                &params.profile_id,
                now_ms,
                preparation,
            )?;
        }

        let allocated: StepAttemptResponse = context
            .sdk
            .attempts
            .invoke_projected(&StepAttemptCommand::AllocateIdentity {
                root_execution_id,
                execution_id: request.execution_id.clone(),
                parent_attempt_id: request.parent_attempt_id.clone(),
                policy_revision: params.policy.revision.clone(),
                kind,
            })
            .map_err(|error| format!("invocation attempt allocation failed: {error}"))?;
        let StepAttemptResponse::Attribution { attribution } = allocated else {
            return Err(
                "step attempt service returned a non-attribution allocation response".into(),
            );
        };

        let task = params.intent.derive_task(&preparation);
        let step = StepRunnerCommand::Run {
            request: PlannedStepRequest {
                attribution,
                profile_id: params.profile_id,
                callable_id: request.callable_id,
                input: request.input,
                tools: request.tools,
                policy: params.policy,
                task,
                context_candidates: preparation.candidates,
                cache_epoch: preparation.projection.cache_epoch,
                route_policy: params.route_policy,
                now_ms,
            },
        };
        let encoded = context
            .kernel
            .encode_value(&step)
            .map_err(|error| error.to_string())?;
        self.runner.invoke(&step_runner_service(), &encoded, host)
    }
}

fn prepare_invocation_context(
    context: &InvocationContext<'_, '_>,
    request: &InvocationRequest,
) -> Result<ContextInvocationPreparation, String> {
    let prepared: ContextResponse = context
        .sdk
        .context
        .invoke_projected(&ContextCommand::PrepareInvocation {
            execution_id: request.execution_id.clone(),
            input: request.input.clone(),
        })
        .map_err(|error| format!("invocation context preparation failed: {error}"))?;
    let ContextResponse::InvocationPrepared { preparation } = prepared else {
        return Err("context service returned a non-preparation response".into());
    };
    Ok(preparation)
}

fn recover_invocation_context(
    context: &InvocationContext<'_, '_>,
    request: &InvocationRequest,
    profile_id: &phenix_core::RoutingProfileId,
    now_ms: u64,
    preparation: ContextInvocationPreparation,
) -> Result<ContextInvocationPreparation, String> {
    let projected: ContextResponse = context
        .sdk
        .context
        .invoke_projected(&ContextCommand::Project {
            execution_id: request.execution_id.clone(),
        })
        .map_err(|error| format!("context recovery projection failed: {error}"))?;
    let ContextResponse::Projection { projection } = projected else {
        return Err("context service returned a non-projection response during recovery".into());
    };
    let anchors = projection
        .entries
        .iter()
        .take(32)
        .map(|entry| ContextAnchor::Resource {
            service: context_service(),
            resource: entry.resource.descriptor.resource_id.as_str().to_owned(),
        })
        .collect::<Vec<_>>();
    let state = phenix_sdk::ContextRecoveryState {
        anchors: anchors.clone(),
        has_durable_session_history: false,
        has_explicit_resource: !projection.entries.is_empty(),
    };
    let prompt = String::from_utf8_lossy(request.input.as_ref()).into_owned();
    let assessed: ContextRecoveryResponse =
        match context
            .sdk
            .recovery
            .invoke_projected(&ContextRecoveryCommand::Assess {
                request: ContextRecoveryRequest {
                    profile_id: profile_id.clone(),
                    prompt: prompt.clone(),
                    state,
                    at: now_ms,
                },
            }) {
            Ok(response) => response,
            Err(ComponentInvocationError::UnboundImport { .. }) => return Ok(preparation),
            Err(error) => return Err(format!("context recovery assessment failed: {error}")),
        };
    let ContextRecoveryResponse::Decision { decision } = assessed;
    let ContextRecoveryDecision::Missing { needs } = decision else {
        return Ok(preparation);
    };

    let recall: MemoryContextResponse =
        match context
            .sdk
            .memory_context
            .invoke_projected(&MemoryContextCommand::Recall {
                request: MemoryContextRecallRequest {
                    request_id: format!("recovery:{}:{now_ms}", request.execution_id),
                    scopes: vec![MemoryScope::Global],
                    prompt,
                    known: anchors,
                    needs: needs.clone(),
                    at: now_ms,
                    limit: 8,
                },
            }) {
            Ok(response) => response,
            Err(ComponentInvocationError::UnboundImport { .. }) => return Ok(preparation),
            Err(error) => return Err(format!("memory context recall failed: {error}")),
        };
    let MemoryContextResponse::Recall {
        candidates,
        completeness,
    } = recall
    else {
        return Err("memory context service returned a non-recall response".into());
    };
    if candidates.is_empty() {
        return Ok(preparation);
    }
    let evidence = candidates
        .into_iter()
        .map(|candidate| RecallEvidence {
            query_relevant: candidate.evidence_class() >= 2,
            live_validated: true,
            candidate,
            resolved_needs: needs.clone(),
            missing_needs: Vec::new(),
            completeness: completeness.clone(),
        })
        .collect();
    let resolved: MemoryContextResponse = context
        .sdk
        .memory_context
        .invoke_projected(&MemoryContextCommand::Resolve { evidence })
        .map_err(|error| format!("memory context resolution failed: {error}"))?;
    let MemoryContextResponse::Resolution { resolution } = resolved else {
        return Err("memory context service returned a non-resolution response".into());
    };
    let RecallResolution::Unique { winner } = resolution else {
        return Ok(preparation);
    };

    let memory: MemoryResponse = match context.sdk.memory.invoke_projected(&MemoryCommand::Get {
        id: winner.candidate.memory_id.clone(),
    }) {
        Ok(response) => response,
        Err(ComponentInvocationError::UnboundImport { .. }) => return Ok(preparation),
        Err(error) => return Err(format!("recovered memory lookup failed: {error}")),
    };
    let MemoryResponse::Memory {
        record: Some(record),
    } = memory
    else {
        return Ok(preparation);
    };
    let source = format!("memory:{}", record.id);
    let resource_id = ContextResourceId::parse(source.clone())
        .map_err(|error| format!("recovered memory id is not a context resource id: {error}"))?;
    let registered: ContextResponse = context
        .sdk
        .context
        .invoke_projected(&ContextCommand::Register {
            resource_id: resource_id.clone(),
            kind: ContextResourceKind::External,
            source,
            scope: ContextScope::Workspace,
            content: record.content.into_bytes().into(),
        })
        .map_err(|error| format!("recovered memory registration failed: {error}"))?;
    let ContextResponse::Registered { resource } = registered else {
        return Err(
            "context service returned a non-registration response for recovered memory".into(),
        );
    };
    let loaded: ContextResponse = context
        .sdk
        .context
        .invoke_projected(&ContextCommand::Load {
            execution_id: request.execution_id.clone(),
            resource_id,
            revision: resource.descriptor.revision,
            requester: ContextInjectionRequester::ContextPolicy,
            lifetime: ContextInjectionLifetime::Execution,
            reason: "fall-through memory recovery".into(),
        })
        .map_err(|error| format!("recovered memory injection failed: {error}"))?;
    if !matches!(loaded, ContextResponse::Loaded { .. }) {
        return Err("context service returned a non-load response for recovered memory".into());
    }
    prepare_invocation_context(context, request)
}

impl PluginInstance for InvocationPackage {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.runner.start(host)
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service == &helper_invocation_service() {
            let context = invocation_context(host, helper_invocation_component_id());
            let command = context
                .kernel
                .decode_projected::<HelperInvocationCommand>(
                    &HelperInvocationInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let HelperInvocationCommand::Invoke { request } = command;
            let resolved: InvocationDefaultsResponse = context
                .sdk
                .defaults
                .invoke_projected(&InvocationDefaultsCommand::ResolveHelper {
                    request: request.clone(),
                })
                .map_err(|error| format!("helper invocation parameters unavailable: {error}"))?;
            let InvocationDefaultsResponse::Params { params } = resolved;
            let kind = request.kind.usage_kind();
            let encoded = self.invoke_with_kind(
                &context,
                host,
                request.as_invocation_request(),
                params,
                kind,
            )?;
            let value: PhenixValue =
                serde_json::from_slice(&encoded).map_err(|error| error.to_string())?;
            let response: StepRunnerResponse =
                value.project().map_err(|error| error.to_string())?;
            let StepRunnerResponse::Completed {
                output, tool_calls, ..
            } = response;
            return context
                .kernel
                .encode_value(&HelperInvocationResponse { output, tool_calls })
                .map_err(|error| error.to_string());
        }

        let context = invocation_context(host, step_runner_component_id());
        if service == &invocation_service() {
            let command = context
                .kernel
                .decode_projected::<InvocationCommand>(&InvocationInterface::interface_id(), input)
                .map_err(|error| error.to_string())?;
            let InvocationCommand::Invoke { request, params } = command;
            return self.invoke_explicit(&context, host, request, params);
        }
        if service == &default_invocation_service() {
            let command = context
                .kernel
                .decode_projected::<DefaultInvocationCommand>(
                    &DefaultInvocationInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let DefaultInvocationCommand::Invoke { request } = command;
            let resolved: InvocationDefaultsResponse = context
                .sdk
                .defaults
                .invoke_projected(&InvocationDefaultsCommand::Resolve {
                    request: request.clone(),
                })
                .map_err(|error| format!("default invocation parameters unavailable: {error}"))?;
            let InvocationDefaultsResponse::Params { params } = resolved;
            return self.invoke_explicit(&context, host, request, params);
        }
        self.runner.invoke(service, input, host)
    }

    fn stop(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        self.runner.stop(host)
    }
}

fn is_isolated_helper(kind: UsageAttemptKind) -> bool {
    matches!(
        kind,
        UsageAttemptKind::Helper
            | UsageAttemptKind::Verification
            | UsageAttemptKind::RecoveryClassifier
    )
}

fn root_execution_id(
    context: &InvocationContext<'_, '_>,
    execution_id: &str,
) -> Result<String, String> {
    if execution_id.trim().is_empty() {
        return Err("invocation execution id must not be empty".into());
    }
    let mut current = execution_id.to_owned();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return Err("execution parent lineage contains a cycle".into());
        }
        let response: ExecutionResponse = context
            .sdk
            .execution
            .invoke_projected(&ExecutionCommand::GetExecution {
                id: current.clone(),
            })
            .map_err(|error| format!("execution lookup failed: {error}"))?;
        let ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } = response
        else {
            return Err(format!("unknown invocation execution: {current}"));
        };
        match execution.parent_execution {
            Some(parent) => current = parent,
            None => return Ok(execution.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_package_splits_helper_from_central_invocation_component() {
        let authority = Authority::default();
        let manifest = step_runner_manifest(authority.clone());
        assert!(manifest.dependencies.is_empty());
        for service in [
            invocation_service(),
            default_invocation_service(),
            helper_invocation_service(),
            step_runner_service(),
        ] {
            assert!(manifest
                .services
                .iter()
                .any(|contribution| contribution.service == service));
        }

        let central = step_runner_component_manifest(authority.clone());
        for interface in [
            InvocationInterface::interface_id(),
            DefaultInvocationInterface::interface_id(),
            phenix_sdk::StepRunnerInterface::interface_id(),
        ] {
            assert!(central
                .exports
                .iter()
                .any(|export| export.interface == interface));
        }
        assert!(!central
            .exports
            .iter()
            .any(|export| export.interface == HelperInvocationInterface::interface_id()));
        assert!(central.imports.iter().any(|import| {
            import.interface == ContextRecoveryInterface::interface_id() && !import.required
        }));
        assert!(central.imports.iter().any(|import| {
            import.interface == MemoryContextInterface::interface_id() && !import.required
        }));

        let helper = helper_invocation_component_manifest(authority);
        assert_eq!(helper.id, helper_invocation_component_id());
        assert_eq!(helper.exports.len(), 1);
        assert_eq!(
            helper.exports[0].interface,
            HelperInvocationInterface::interface_id()
        );
        assert!(!helper
            .imports
            .iter()
            .any(|import| import.interface == ContextInterface::interface_id()));
        assert!(!helper.imports.iter().any(|import| {
            import.interface == MemoryContextInterface::interface_id()
                || import.interface == MemoryInterface::interface_id()
        }));
    }

    #[test]
    fn context_service_identity_is_not_redefined_by_invocation_package() {
        assert_eq!(context_service(), phenix_sdk::context_service());
    }
}
