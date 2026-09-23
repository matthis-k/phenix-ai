pub use phenix_core::{
    model_inference_service, ModelInferenceFailure, ModelInferenceRequest, ModelInferenceResponse,
    MODEL_INFERENCE_SERVICE,
};
use phenix_core::{
    Authority, CapabilityId, ComponentInterface, DurableSchema, InvocationOutcome, KernelError,
    PhenixValue, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, Project, ResourceNamespace, RoutingProfileId, ServiceContribution, ServiceId,
    TransactionOp,
};
pub use phenix_sdk::{
    model_diagnostic_event_type, model_dispatch_service, model_routing_service, ModelCommand,
    ModelDiagnosticEvent, ModelDispatchCommand, ModelDispatchFailure, ModelDispatchInterface,
    ModelDispatchResponse, ModelResponse, ModelRoutingInterface, ModelTarget, PreparedDispatch,
    RoutingProfile, RoutingProfileDescriptor, MODEL_DIAGNOSTIC_EVENT_VERSION,
    MODEL_DISPATCH_SERVICE, MODEL_ROUTING_SERVICE,
};
use std::collections::BTreeSet;

use crate::routing_service::{RoutingServiceState, ROUTING_RUNTIME_KEY};

mod packaged;

const MODEL_ROUTING_PLUGIN: &str = "phenix.models";
const MODEL_NAMESPACE: &str = "phenix.models.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const PROFILE_INDEX: &str = "index/profiles";

type ModelContext<'host, 'runtime, 'state> =
    PluginContext<'host, 'runtime, (), (), &'state mut BTreeSet<PluginId>>;

fn context<'host, 'runtime, 'state>(
    host: &'host PluginHost<'runtime>,
    authenticated: &'state mut BTreeSet<PluginId>,
) -> ModelContext<'host, 'runtime, 'state> {
    PluginContext::new(host, (), (), authenticated)
}

#[must_use]
pub fn model_routing_manifest(maximum_authority: Authority) -> PluginManifest {
    let persistence = Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ]);
    let maximum_authority = Authority::new(
        maximum_authority
            .capabilities()
            .cloned()
            .chain(persistence.capabilities().cloned()),
    );
    PluginManifest {
        id: PluginId::parse(MODEL_ROUTING_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: [model_routing_service(), model_dispatch_service()]
            .into_iter()
            .map(|service| ServiceContribution {
                role: phenix_core::ServiceRole::Terminal,
                service,
                priority: 100,
                required_authority: Authority::default(),
            })
            .collect(),
        resource_namespaces: vec![model_namespace()],
        maximum_authority,
    }
}

#[must_use]
pub fn model_routing_factory() -> Box<dyn PluginInstance> {
    Box::new(ModelRoutingPlugin::default())
}

fn model_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(MODEL_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

#[derive(Default)]
struct ModelRoutingPlugin {
    authenticated: BTreeSet<PluginId>,
    routing: RoutingServiceState,
}

impl PluginInstance for ModelRoutingPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let snapshot = {
            let context = context(host, &mut self.authenticated);
            context
                .kernel
                .register_durable_schema(&DurableSchema::new(model_namespace(), 1))
                .map_err(|error| error.to_string())?;
            context
                .kernel
                .read_durable(&model_namespace(), ROUTING_RUNTIME_KEY)
                .map_err(|error| error.to_string())?
        };
        self.routing = RoutingServiceState::restore(snapshot.as_deref())
            .map_err(|error| format!("invalid durable routing state: {error:?}"))?;
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let mut context = context(host, &mut self.authenticated);
        if service == &model_routing_service() {
            let command = context
                .kernel
                .decode_projected::<ModelCommand>(&ModelRoutingInterface::interface_id(), input)
                .map_err(|error| error.to_string())?;
            let response = handle_routing(&mut context, &mut self.routing, command)?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &model_dispatch_service() {
            let command = context
                .kernel
                .decode_projected::<ModelDispatchCommand>(
                    &ModelDispatchInterface::interface_id(),
                    input,
                )
                .map_err(|error| error.to_string())?;
            let outcome = match handle_dispatch(&mut context, &self.routing, command) {
                Ok(response) => InvocationOutcome::success(PhenixValue::from(&response)),
                Err(error) => InvocationOutcome::domain_error(PhenixValue::from(&error)),
            };
            return serde_json::to_vec(&outcome.into_transport_value())
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported model service: {service}"))
    }
}

fn handle_routing(
    context: &mut ModelContext<'_, '_, '_>,
    routing: &mut RoutingServiceState,
    command: ModelCommand,
) -> Result<ModelResponse, String> {
    let mutates_runtime = matches!(
        &command,
        ModelCommand::PublishCapabilities { .. } | ModelCommand::RecordEvidence { .. }
    );
    let previous_runtime = if mutates_runtime {
        read_raw(context, ROUTING_RUNTIME_KEY)?
    } else {
        None
    };

    if let Some(response) =
        routing.handle_state_command(command.clone(), |id| read_profile(context, id))
    {
        let response = response?;
        if mutates_runtime {
            persist_runtime_state(context, routing, previous_runtime)?;
        }
        emit_routing_diagnostic(context, &command, &response);
        return Ok(response);
    }

    match command {
        ModelCommand::PreparePackagedProfiles { profiles } => packaged::prepare(context, profiles),
        ModelCommand::RegisterProfile { profile } => {
            insert_profile(context, &profile)?;
            Ok(ModelResponse::Profile {
                profile: Some(profile),
            })
        }
        ModelCommand::ReplaceProfile { expected, profile } => {
            replace_profile(context, &expected, &profile)?;
            Ok(ModelResponse::Profile {
                profile: Some(profile),
            })
        }
        ModelCommand::GetProfile { id } => Ok(ModelResponse::Profile {
            profile: read_profile(context, &id)?,
        }),
        ModelCommand::ListProfiles => {
            let retired = packaged::retired(context)?;
            Ok(ModelResponse::Profiles {
                profiles: load_profiles(context)?
                    .into_iter()
                    .filter(|profile| !retired.contains(&profile.id))
                    .map(|profile| descriptor(&profile))
                    .collect(),
            })
        }
        ModelCommand::SetProviderAuthenticated {
            provider_plugin,
            authenticated,
        } => {
            let changed = if authenticated {
                context.plugin.state.insert(provider_plugin.clone())
            } else {
                context.plugin.state.remove(&provider_plugin)
            };
            if changed {
                emit_diagnostic(
                    context,
                    ModelDiagnosticEvent::AuthenticationChanged {
                        provider_plugin: provider_plugin.as_str().to_owned(),
                        authenticated,
                        authenticated_providers: authenticated_providers(context),
                    },
                );
            }
            Ok(ModelResponse::Authentication {
                provider_plugin,
                authenticated,
            })
        }
        ModelCommand::PublishCapabilities { .. }
        | ModelCommand::ListCandidates { .. }
        | ModelCommand::ResolveWithRequirements { .. }
        | ModelCommand::RecordEvidence { .. } => {
            Err("routing state command was not handled".into())
        }
    }
}

fn handle_dispatch(
    context: &mut ModelContext<'_, '_, '_>,
    routing: &RoutingServiceState,
    command: ModelDispatchCommand,
) -> Result<ModelDispatchResponse, ModelDispatchFailure> {
    match command {
        ModelDispatchCommand::PrepareResolved {
            decision,
            input,
            tools,
            continuation,
        } => {
            let authenticated = context
                .plugin
                .state
                .contains(&decision.target.provider_plugin);
            emit_diagnostic(
                context,
                ModelDiagnosticEvent::DispatchPreflight {
                    provider_plugin: decision.target.provider_plugin.as_str().to_owned(),
                    model: decision.target.model.as_str().to_owned(),
                    authenticated,
                    authenticated_providers: authenticated_providers(context),
                    candidate_ordinal: decision.candidate_ordinal,
                    policy_revision: decision.policy_revision.clone(),
                    capability_generation: decision.capability_generation.as_str().to_owned(),
                    input_bytes: input.as_ref().len(),
                    tool_count: tools.len(),
                    continuation_turns: continuation.len(),
                },
            );
            if let Err(failure) = validate_dispatch(context, routing, &decision) {
                emit_diagnostic(
                    context,
                    ModelDiagnosticEvent::DispatchPreflightRejected {
                        provider_plugin: decision.target.provider_plugin.as_str().to_owned(),
                        model: decision.target.model.as_str().to_owned(),
                        authenticated,
                        authenticated_providers: authenticated_providers(context),
                        reason: failure.message().to_owned(),
                    },
                );
                return Err(ModelDispatchFailure { decision, failure });
            }
            let request = encode_request(context, &decision.target, input, tools, continuation)
                .map_err(|failure| ModelDispatchFailure {
                    decision: decision.clone(),
                    failure,
                })?;
            emit_diagnostic(
                context,
                ModelDiagnosticEvent::DispatchPrepared {
                    provider_plugin: decision.target.provider_plugin.as_str().to_owned(),
                    model: decision.target.model.as_str().to_owned(),
                    request_bytes: request.as_ref().len(),
                },
            );
            Ok(ModelDispatchResponse::Ready {
                prepared: PreparedDispatch::new(decision, request),
            })
        }
        ModelDispatchCommand::InvokePrepared { prepared } => {
            let (decision, request) = prepared.into_parts();
            emit_diagnostic(
                context,
                ModelDiagnosticEvent::DispatchInvocationStarted {
                    provider_plugin: decision.target.provider_plugin.as_str().to_owned(),
                    model: decision.target.model.as_str().to_owned(),
                    request_bytes: request.as_ref().len(),
                },
            );
            let response = match invoke_encoded_target(context, &decision.target, request) {
                Ok(response) => response,
                Err(failure) => {
                    emit_diagnostic(
                        context,
                        ModelDiagnosticEvent::DispatchInvocationFailed {
                            provider_plugin: decision.target.provider_plugin.as_str().to_owned(),
                            model: decision.target.model.as_str().to_owned(),
                            reason: failure.message().to_owned(),
                        },
                    );
                    return Err(ModelDispatchFailure { decision, failure });
                }
            };
            emit_diagnostic(
                context,
                ModelDiagnosticEvent::DispatchInvocationSucceeded {
                    provider_plugin: decision.target.provider_plugin.as_str().to_owned(),
                    model: decision.target.model.as_str().to_owned(),
                },
            );
            Ok(ModelDispatchResponse::Inference { decision, response })
        }
    }
}

fn authenticated_providers(context: &ModelContext<'_, '_, '_>) -> Vec<String> {
    context
        .plugin
        .state
        .iter()
        .map(|provider| provider.as_str().to_owned())
        .collect()
}

fn emit_routing_diagnostic(
    context: &ModelContext<'_, '_, '_>,
    command: &ModelCommand,
    response: &ModelResponse,
) {
    let (
        ModelCommand::ResolveWithRequirements {
            profile_id,
            callable_id,
            ..
        },
        ModelResponse::Decision { selection },
    ) = (command, response)
    else {
        return;
    };
    emit_diagnostic(
        context,
        ModelDiagnosticEvent::RoutingDecision {
            profile_id: profile_id.as_str().to_owned(),
            callable_id: callable_id
                .as_ref()
                .map(|callable| callable.as_str().to_owned()),
            provider_plugin: selection
                .decision
                .target
                .provider_plugin
                .as_str()
                .to_owned(),
            model: selection.decision.target.model.as_str().to_owned(),
            candidate_ordinal: selection.decision.candidate_ordinal,
            rejected_candidates: selection.rejected.len(),
        },
    );
}

fn emit_diagnostic(context: &ModelContext<'_, '_, '_>, diagnostic: ModelDiagnosticEvent) {
    let Ok(payload) = serde_json::to_vec(&diagnostic) else {
        return;
    };
    let _ = context.kernel.dispatch_event(
        model_diagnostic_event_type(),
        MODEL_DIAGNOSTIC_EVENT_VERSION,
        0,
        0,
        payload,
    );
}

fn validate_dispatch(
    context: &ModelContext<'_, '_, '_>,
    routing: &RoutingServiceState,
    decision: &phenix_sdk::RouteDecision,
) -> Result<(), ModelInferenceFailure> {
    routing
        .validate_decision(decision)
        .map_err(|message| ModelInferenceFailure::InvalidRequest { message })?;
    ensure_authenticated(context, &decision.target.provider_plugin)
}

fn ensure_authenticated(
    context: &ModelContext<'_, '_, '_>,
    provider_plugin: &PluginId,
) -> Result<(), ModelInferenceFailure> {
    if context.plugin.state.contains(provider_plugin) {
        Ok(())
    } else {
        Err(ModelInferenceFailure::Authentication {
            message: format!("provider authentication required: {provider_plugin}"),
        })
    }
}

fn encode_request(
    context: &ModelContext<'_, '_, '_>,
    target: &ModelTarget,
    input: phenix_core::Bytes,
    tools: Vec<phenix_core::ModelToolDescriptor>,
    continuation: Vec<phenix_core::ModelToolTurn>,
) -> Result<phenix_core::Bytes, ModelInferenceFailure> {
    let request = ModelInferenceRequest {
        model: target.model.clone(),
        input,
        options: target.options.clone(),
        tools,
        continuation,
    };
    context
        .kernel
        .encode_value(&request)
        .map(Into::into)
        .map_err(|error| ModelInferenceFailure::InvalidRequest {
            message: error.to_string(),
        })
}

fn invoke_encoded_target(
    context: &mut ModelContext<'_, '_, '_>,
    target: &ModelTarget,
    request: phenix_core::Bytes,
) -> Result<ModelInferenceResponse, ModelInferenceFailure> {
    let output = context
        .kernel
        .invoke_service_abi(
            &model_inference_service(),
            request.as_ref(),
            context.call.authority,
            Some(&target.provider_plugin),
        )
        .map_err(kernel_model_failure)?;
    let value: PhenixValue =
        serde_json::from_slice(&output).map_err(|error| ModelInferenceFailure::Protocol {
            message: format!("cannot decode model inference outcome: {error}"),
        })?;
    match InvocationOutcome::from_transport_value(value) {
        InvocationOutcome::Success(value) => ModelInferenceResponse::try_from(Project(&value))
            .map_err(|error| ModelInferenceFailure::Protocol {
                message: format!("cannot project model inference response: {error}"),
            }),
        InvocationOutcome::DomainError(value) => {
            let failure = ModelInferenceFailure::try_from(Project(&value)).map_err(|error| {
                ModelInferenceFailure::Protocol {
                    message: format!("cannot project model inference failure: {error}"),
                }
            })?;
            Err(failure)
        }
    }
}

fn kernel_model_failure(error: KernelError) -> ModelInferenceFailure {
    let message = error.to_string();
    match error {
        KernelError::ServiceDenied { .. } | KernelError::HostOperationDenied { .. } => {
            ModelInferenceFailure::Permission { message }
        }
        KernelError::NoEligibleProvider(_)
        | KernelError::BoundProviderUnavailable { .. }
        | KernelError::PluginNotActive(_)
        | KernelError::RuntimeProviderUnavailable(_)
        | KernelError::RuntimeProviderNotExecutable { .. }
        | KernelError::RuntimeProviderContractUnavailable { .. }
        | KernelError::ServiceInvoke { .. } => ModelInferenceFailure::Unavailable { message },
        _ => ModelInferenceFailure::Protocol { message },
    }
}

fn persist_runtime_state(
    context: &ModelContext<'_, '_, '_>,
    routing: &mut RoutingServiceState,
    previous: Option<Vec<u8>>,
) -> Result<(), String> {
    let next = match routing.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            *routing = RoutingServiceState::restore(previous.as_deref())
                .map_err(|restore| format!("routing state rollback failed: {restore:?}"))?;
            return Err(format!("routing state snapshot failed: {error:?}"));
        }
    };
    let result = context.kernel.transact_durable(
        &model_namespace(),
        &[
            TransactionOp::AssertValue {
                key: ROUTING_RUNTIME_KEY.into(),
                expected: previous.clone(),
            },
            TransactionOp::Put {
                key: ROUTING_RUNTIME_KEY.into(),
                value: next,
            },
        ],
    );
    if let Err(error) = result {
        *routing = RoutingServiceState::restore(previous.as_deref())
            .map_err(|restore| format!("routing state rollback failed: {restore:?}"))?;
        return Err(error.to_string());
    }
    Ok(())
}

fn descriptor(profile: &RoutingProfile) -> RoutingProfileDescriptor {
    let mut providers = BTreeSet::from([profile.default_target.provider_plugin.clone()]);
    providers.extend(
        profile
            .fallback_targets
            .iter()
            .chain(profile.callable_targets.values())
            .map(|target| target.provider_plugin.clone()),
    );
    RoutingProfileDescriptor {
        id: profile.id.clone(),
        providers: providers.into_iter().collect(),
    }
}

fn insert_profile(
    context: &ModelContext<'_, '_, '_>,
    profile: &RoutingProfile,
) -> Result<(), String> {
    let key = profile_key(&profile.id);
    let old_index = read_raw(context, PROFILE_INDEX)?;
    let mut ids: Vec<RoutingProfileId> = old_index
        .as_deref()
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    if ids.contains(&profile.id) || read_raw(context, &key)?.is_some() {
        return Err(format!(
            "routing profile already registered: {}",
            profile.id
        ));
    }
    ids.push(profile.id.clone());
    ids.sort();
    context
        .kernel
        .transact_durable(
            &model_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: PROFILE_INDEX.into(),
                    expected: old_index,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(profile).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: PROFILE_INDEX.into(),
                    value: serde_json::to_vec(&ids).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn replace_profile(
    context: &ModelContext<'_, '_, '_>,
    expected: &RoutingProfile,
    replacement: &RoutingProfile,
) -> Result<(), String> {
    if expected.id != replacement.id {
        return Err(format!(
            "routing profile replacement identity mismatch: {} != {}",
            expected.id, replacement.id
        ));
    }

    let key = profile_key(&expected.id);
    let Some(current_raw) = read_raw(context, &key)? else {
        return Err(format!(
            "routing profile replacement target is missing: {}",
            expected.id
        ));
    };
    let current: RoutingProfile =
        serde_json::from_slice(&current_raw).map_err(|error| error.to_string())?;
    if current != *expected {
        return Err(format!(
            "routing profile replacement conflict: {}",
            expected.id
        ));
    }

    context
        .kernel
        .transact_durable(
            &model_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: Some(current_raw),
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(replacement).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_profile(
    context: &ModelContext<'_, '_, '_>,
    id: &RoutingProfileId,
) -> Result<Option<RoutingProfile>, String> {
    read_raw(context, &profile_key(id))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn load_profiles(context: &ModelContext<'_, '_, '_>) -> Result<Vec<RoutingProfile>, String> {
    let ids: Vec<RoutingProfileId> = read_raw(context, PROFILE_INDEX)?
        .as_deref()
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or_default();
    ids.into_iter()
        .map(|id| {
            read_profile(context, &id)?.ok_or_else(|| format!("missing routing profile: {id}"))
        })
        .collect()
}

fn read_raw(context: &ModelContext<'_, '_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&model_namespace(), key)
        .map_err(|error| error.to_string())
}

fn profile_key(id: &RoutingProfileId) -> String {
    format!("profile/{id}")
}

#[cfg(test)]
mod tests;
