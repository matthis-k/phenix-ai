use crate::{
    assemble_prompt, context_component_id,
    projection_state::ContextProjectionState,
    state_service::{ContextStateService, CONTEXT_PROJECTION_STATE_KEY},
    PromptSection, PromptSectionKind,
};
use phenix_core::{
    Authority, Bytes, CapabilityId, ComponentInterface, ContextResourceId, ContextRevisionId,
    DurableSchema, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResourceNamespace, SdkClient, ServiceContribution, ServiceId, TransactionOp,
};
use phenix_sdk::{
    choose_cache_aware_compaction, context_service, AdmittedContextItem, CachePlacement,
    ContextCandidate, ContextCommand, ContextDescriptor, ContextInjection,
    ContextInjectionLifetime, ContextInjectionRequester, ContextInterface,
    ContextInvocationMaterialization, ContextInvocationPreparation, ContextProjectionForm,
    ContextResourceKind, ContextResourceRevision, ContextResponse, ContextRetention, ContextScope,
    ContextSource, ExactContextReference, ExecutionCommand, ExecutionContextProjection,
    ExecutionInterface, ExecutionResourceCommand, ExecutionResourceInterface,
    ExecutionResourceResponse, ExecutionResponse, ExecutionState, ProjectedContextEntry,
    ProjectionCheckpoint, ProjectionRevision, RepositoryContextSource, WorkerTaskState,
};
use sha2::{Digest, Sha256};

const CONTEXT_PLUGIN: &str = "phenix.context";
const CONTEXT_NAMESPACE: &str = "phenix.context.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const ALL_RESOURCES_KEY: &str = "resources/@all";

struct ContextSdk<'host, 'runtime> {
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
    resources: SdkClient<'host, 'runtime, ExecutionResourceInterface>,
}

type ContextPluginContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, ContextSdk<'host, 'runtime>>;

fn context<'host, 'runtime>(
    host: &'host PluginHost<'runtime>,
) -> ContextPluginContext<'host, 'runtime> {
    PluginContext::new(
        host,
        ContextSdk {
            execution: SdkClient::new(host, context_component_id()),
            resources: SdkClient::new(host, context_component_id()),
        },
        (),
        (),
    )
}

#[must_use]
pub fn context_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(CONTEXT_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: context_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![context_namespace()],
        maximum_authority: Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
        ]),
    }
}

#[must_use]
pub fn context_factory() -> Box<dyn PluginInstance> {
    Box::new(ContextPlugin {
        state: ContextStateService::default(),
    })
}

pub(crate) fn context_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(CONTEXT_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

struct ContextPlugin {
    state: ContextStateService,
}

impl PluginInstance for ContextPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        let context = context(host);
        context
            .kernel
            .register_durable_schema(&DurableSchema::new(context_namespace(), 1))
            .map_err(|error| error.to_string())?;
        let snapshot = context
            .kernel
            .read_durable(&context_namespace(), CONTEXT_PROJECTION_STATE_KEY)
            .map_err(|error| error.to_string())?;
        self.state = ContextStateService::restore(snapshot.as_deref())
            .map_err(|error| format!("invalid durable context projection state: {error:?}"))?;
        Ok(())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &context_service() {
            return Err(format!("unsupported context service: {service}"));
        }
        let context = context(host);
        let command = context
            .kernel
            .decode_projected::<ContextCommand>(&ContextInterface::interface_id(), input)
            .map_err(|error| error.to_string())?;
        let response = handle(&context, &mut self.state, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
    if let Some(execution_id) = state_command_execution(&command) {
        require_active_execution(context, execution_id)?;
        return handle_state_command(context, state, command);
    }

    match command {
        ContextCommand::Register {
            resource_id,
            kind,
            source,
            scope,
            content,
        } => Ok(ContextResponse::Registered {
            resource: register_resource(context, resource_id, kind, source, scope, content)?,
        }),
        ContextCommand::Get {
            resource_id,
            revision,
        } => Ok(ContextResponse::Resource {
            resource: read_resource(context, &resource_id, &revision)?,
        }),
        ContextCommand::List => Ok(ContextResponse::Resources {
            descriptors: list_descriptors(context)?,
        }),
        ContextCommand::DiscoverRepository {
            workspace_id,
            sources,
        } => Ok(ContextResponse::Discovered {
            descriptors: discover_repository(context, &workspace_id, sources)?,
        }),
        ContextCommand::Load {
            execution_id,
            resource_id,
            revision,
            requester,
            lifetime,
            reason,
        } => {
            let (injection, resource) = load_context(
                context,
                state,
                execution_id,
                resource_id,
                revision,
                requester,
                lifetime,
                reason,
            )?;
            Ok(ContextResponse::Loaded {
                injection,
                resource,
            })
        }
        ContextCommand::LoadDelegatedResult { task_id } => {
            let (injection, resource) = load_delegated_result(context, state, task_id)?;
            Ok(ContextResponse::Loaded {
                injection,
                resource,
            })
        }
        ContextCommand::LoadOnce {
            admission_id,
            execution_id,
            resource_id,
            revision,
            requester,
            lifetime,
            reason,
        } => {
            let (injection, resource) = load_context_once(
                context,
                state,
                admission_id,
                execution_id,
                resource_id,
                revision,
                requester,
                lifetime,
                reason,
            )?;
            Ok(ContextResponse::Loaded {
                injection,
                resource,
            })
        }
        ContextCommand::Project { execution_id } => Ok(ContextResponse::Projection {
            projection: project_context(context, execution_id)?,
        }),
        ContextCommand::PrepareInvocation {
            execution_id,
            input,
        } => Ok(ContextResponse::InvocationPrepared {
            preparation: prepare_invocation(context, state, execution_id, input)?,
        }),
        ContextCommand::MaterializeInvocation {
            execution_id,
            input,
            expected_projection,
        } => Ok(ContextResponse::InvocationMaterialized {
            materialization: materialize_invocation(
                context,
                state,
                execution_id,
                input,
                expected_projection,
            )?,
        }),
        ContextCommand::EvaluateCompactionCost { request } => {
            let decision = choose_cache_aware_compaction(&request)
                .map_err(|error| format!("cache compaction cost evaluation failed: {error:?}"))?;
            Ok(ContextResponse::CompactionCostDecision { decision })
        }
        ContextCommand::GetProjectionState { .. }
        | ContextCommand::Admit { .. }
        | ContextCommand::PrepareCompaction { .. }
        | ContextCommand::CommitCompaction { .. }
        | ContextCommand::InvalidateProjection { .. } => {
            Err("context projection command leaked past state dispatcher".into())
        }
    }
}

fn state_command_execution(command: &ContextCommand) -> Option<&str> {
    match command {
        ContextCommand::Admit { request } => Some(&request.execution_id),
        ContextCommand::PrepareCompaction { proposal } => Some(&proposal.execution_id),
        ContextCommand::GetProjectionState { execution_id }
        | ContextCommand::CommitCompaction { execution_id, .. }
        | ContextCommand::InvalidateProjection { execution_id } => Some(execution_id),
        _ => None,
    }
}

fn handle_state_command(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
    if matches!(&command, ContextCommand::GetProjectionState { .. }) {
        return state
            .handle_state_command(command)
            .ok_or_else(|| "projection state command leaked past state service".to_owned())?;
    }

    let previous = read_raw(context, CONTEXT_PROJECTION_STATE_KEY)?;
    let response = match state.handle_state_command(command) {
        Some(Ok(response)) => response,
        Some(Err(error)) => {
            *state = ContextStateService::restore(previous.as_deref())
                .map_err(|restore| format!("context state rollback failed: {restore:?}"))?;
            return Err(error);
        }
        None => return Err("non-state command reached context state dispatcher".into()),
    };
    persist_state(context, state, previous)?;
    Ok(response)
}

fn persist_state(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    previous: Option<Vec<u8>>,
) -> Result<(), String> {
    let next = match state.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            *state = ContextStateService::restore(previous.as_deref())
                .map_err(|restore| format!("context state rollback failed: {restore:?}"))?;
            return Err(format!("context state snapshot failed: {error:?}"));
        }
    };
    let result = context.kernel.transact_durable(
        &context_namespace(),
        &[
            TransactionOp::AssertValue {
                key: CONTEXT_PROJECTION_STATE_KEY.into(),
                expected: previous.clone(),
            },
            TransactionOp::Put {
                key: CONTEXT_PROJECTION_STATE_KEY.into(),
                value: next,
            },
        ],
    );
    if let Err(error) = result {
        *state = ContextStateService::restore(previous.as_deref())
            .map_err(|restore| format!("context state rollback failed: {restore:?}"))?;
        return Err(error.to_string());
    }
    Ok(())
}

fn register_resource(
    context: &ContextPluginContext<'_, '_>,
    resource_id: ContextResourceId,
    kind: ContextResourceKind,
    source: String,
    scope: ContextScope,
    content: Bytes,
) -> Result<ContextResourceRevision, String> {
    validate_identity("context source", &source)?;
    if let ContextScope::PathPrefix(prefix) = &scope {
        validate_identity("context path scope", prefix)?;
    }
    let revision = content_hash(content.as_ref());
    let estimated_bytes = u64::try_from(content.as_ref().len())
        .map_err(|_| "context resource byte length exceeds u64".to_owned())?;
    let resource = ContextResourceRevision {
        descriptor: ContextDescriptor {
            resource_id: resource_id.clone(),
            revision: revision.clone(),
            kind,
            source,
            scope,
            content_identity: revision.as_str().to_owned(),
            estimated_bytes,
        },
        content,
    };
    let key = resource_key(&resource_id, &revision);
    if let Some(existing) = read_raw(context, &key)? {
        let existing: ContextResourceRevision =
            serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
        if existing != resource {
            return Err(format!(
                "immutable context revision collision: {resource_id}@{revision}"
            ));
        }
        return Ok(existing);
    }
    let old_refs = read_raw(context, ALL_RESOURCES_KEY)?;
    let mut refs = decode_refs(old_refs.as_deref())?;
    refs.push(ExactContextReference {
        resource_id,
        revision,
    });
    refs.sort();
    refs.dedup();
    context
        .kernel
        .transact_durable(
            &context_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: ALL_RESOURCES_KEY.into(),
                    expected: old_refs,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(&resource).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: ALL_RESOURCES_KEY.into(),
                    value: serde_json::to_vec(&refs).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(resource)
}

fn discover_repository(
    context: &ContextPluginContext<'_, '_>,
    workspace_id: &str,
    mut sources: Vec<RepositoryContextSource>,
) -> Result<Vec<ContextDescriptor>, String> {
    validate_identity("workspace id", workspace_id)?;
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    let mut descriptors = Vec::new();
    for source in sources {
        let Some(kind) = project_file_kind(&source.path) else {
            continue;
        };
        let scope = match kind {
            ContextResourceKind::Skill => ContextScope::Workspace,
            _ => match parent_path(&source.path) {
                Some(parent) if !parent.is_empty() => ContextScope::PathPrefix(parent.to_owned()),
                _ => ContextScope::Workspace,
            },
        };
        let prefix = match kind {
            ContextResourceKind::ProjectInstruction => "project-instruction",
            ContextResourceKind::ProjectDocument => "project-document",
            ContextResourceKind::Skill => "skill",
            ContextResourceKind::External => unreachable!(),
        };
        let resource_id =
            ContextResourceId::parse(format!("{prefix}:{workspace_id}:{}", source.path))
                .map_err(str::to_owned)?;
        let resource = register_resource(
            context,
            resource_id,
            kind,
            source.path,
            scope,
            source.content,
        )?;
        descriptors.push(resource.descriptor);
    }
    descriptors.sort_by(|left, right| {
        left.resource_id
            .cmp(&right.resource_id)
            .then_with(|| left.revision.cmp(&right.revision))
    });
    Ok(descriptors)
}

fn project_file_kind(path: &str) -> Option<ContextResourceKind> {
    match file_name(path) {
        "AGENTS.md" | "AGENTS.override.md" => Some(ContextResourceKind::ProjectInstruction),
        "CONTRIBUTING.md" | "DEVELOPMENT.md" => Some(ContextResourceKind::ProjectDocument),
        "SKILL.md" => Some(ContextResourceKind::Skill),
        _ => None,
    }
}

fn load_delegated_result(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    task_id: String,
) -> Result<(ContextInjection, ContextResourceRevision), String> {
    validate_identity("delegated task id", &task_id)?;
    let response: ExecutionResourceResponse = context
        .sdk
        .resources
        .invoke_projected(&ExecutionResourceCommand::GetDelegated {
            task_id: task_id.clone(),
        })
        .map_err(|error| format!("delegated task lookup failed: {error}"))?;
    let ExecutionResourceResponse::DelegatedTaskLookup {
        task: Some(record),
    } = response
    else {
        return Err(format!("unknown delegated task: {task_id}"));
    };
    if !matches!(record.task.state, WorkerTaskState::Completed { .. }) {
        return Err(format!("delegated task is not completed: {task_id}"));
    }
    let result = record
        .result
        .as_ref()
        .ok_or_else(|| format!("completed delegated task has no result: {task_id}"))?;
    let draft = result
        .context_draft(&task_id, &record.binding)
        .map_err(|error| format!("invalid delegated result: {error:?}"))?;
    let resource = register_resource(
        context,
        draft.resource_id.clone(),
        ContextResourceKind::External,
        draft.source,
        ContextScope::Workspace,
        draft.content,
    )?;
    load_context_once(
        context,
        state,
        format!("delegation-result:{task_id}"),
        record.task.parent_execution,
        resource.descriptor.resource_id,
        resource.descriptor.revision,
        ContextInjectionRequester::Orchestration,
        ContextInjectionLifetime::Execution,
        format!("delegated result {task_id}"),
    )
}

fn load_context(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    execution_id: String,
    resource_id: ContextResourceId,
    revision: ContextRevisionId,
    requester: ContextInjectionRequester,
    lifetime: ContextInjectionLifetime,
    reason: String,
) -> Result<(ContextInjection, ContextResourceRevision), String> {
    load_context_internal(
        context,
        state,
        None,
        execution_id,
        resource_id,
        revision,
        requester,
        lifetime,
        reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_context_once(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    admission_id: String,
    execution_id: String,
    resource_id: ContextResourceId,
    revision: ContextRevisionId,
    requester: ContextInjectionRequester,
    lifetime: ContextInjectionLifetime,
    reason: String,
) -> Result<(ContextInjection, ContextResourceRevision), String> {
    validate_identity("context admission id", &admission_id)?;
    load_context_internal(
        context,
        state,
        Some(admission_id),
        execution_id,
        resource_id,
        revision,
        requester,
        lifetime,
        reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn load_context_internal(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    admission_id: Option<String>,
    execution_id: String,
    resource_id: ContextResourceId,
    revision: ContextRevisionId,
    requester: ContextInjectionRequester,
    lifetime: ContextInjectionLifetime,
    reason: String,
) -> Result<(ContextInjection, ContextResourceRevision), String> {
    validate_identity("execution id", &execution_id)?;
    validate_identity("context load reason", &reason)?;
    require_active_execution(context, &execution_id)?;
    let resource = read_resource(context, &resource_id, &revision)?
        .ok_or_else(|| format!("unknown context revision: {resource_id}@{revision}"))?;
    let source = ExactContextReference {
        resource_id,
        revision,
    };

    let receipt = admission_id
        .as_deref()
        .map(|admission_id| injection_admission_key(&execution_id, admission_id));
    if let Some(receipt_key) = receipt.as_deref() {
        if let Some(existing) = read_raw(context, receipt_key)? {
            let existing: ContextInjection =
                serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
            if existing.execution_id != execution_id
                || existing.source != source
                || existing.requester != requester
                || existing.lifetime != lifetime
                || existing.reason != reason
            {
                return Err(format!(
                    "context admission identity reused with changed injection: {}",
                    admission_id
                        .as_deref()
                        .expect("receipt implies admission id")
                ));
            }
            return Ok((existing, resource));
        }
    }

    let key = injections_key(&execution_id);
    let old_injections = read_raw(context, &key)?;
    let mut injections = decode_injections(old_injections.as_deref())?;
    let sequence = u64::try_from(injections.len())
        .map_err(|_| "context injection sequence overflow".to_owned())?
        + 1;
    let injection = ContextInjection {
        sequence,
        execution_id: execution_id.clone(),
        source,
        requester,
        lifetime,
        reason,
    };
    injections.push(injection.clone());

    let old_state = read_raw(context, CONTEXT_PROJECTION_STATE_KEY)?;
    let mut next_state = state.clone();
    let state_changed = next_state.invalidate_if_present(&execution_id).is_some();
    let next_state_bytes = state_changed
        .then(|| {
            next_state
                .snapshot()
                .map_err(|error| format!("context state snapshot failed: {error:?}"))
        })
        .transpose()?;

    let mut operations = vec![
        TransactionOp::AssertValue {
            key: key.clone(),
            expected: old_injections,
        },
        TransactionOp::Put {
            key,
            value: serde_json::to_vec(&injections).map_err(|error| error.to_string())?,
        },
    ];
    if let Some(receipt_key) = receipt {
        operations.push(TransactionOp::AssertValue {
            key: receipt_key.clone(),
            expected: None,
        });
        operations.push(TransactionOp::Put {
            key: receipt_key,
            value: serde_json::to_vec(&injection).map_err(|error| error.to_string())?,
        });
    }
    if let Some(next_state_bytes) = next_state_bytes {
        operations.push(TransactionOp::AssertValue {
            key: CONTEXT_PROJECTION_STATE_KEY.into(),
            expected: old_state,
        });
        operations.push(TransactionOp::Put {
            key: CONTEXT_PROJECTION_STATE_KEY.into(),
            value: next_state_bytes,
        });
    }
    context
        .kernel
        .transact_durable(&context_namespace(), &operations)
        .map_err(|error| error.to_string())?;
    if state_changed {
        *state = next_state;
    }
    Ok((injection, resource))
}

fn require_active_execution(
    context: &ContextPluginContext<'_, '_>,
    execution_id: &str,
) -> Result<(), String> {
    let response = context
        .sdk
        .execution
        .invoke_projected::<ExecutionCommand, ExecutionResponse>(&ExecutionCommand::GetExecution {
            id: execution_id.to_owned(),
        })
        .map_err(|error| error.to_string())?;
    match response {
        ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } if execution.state == ExecutionState::Active => Ok(()),
        ExecutionResponse::ExecutionLookup {
            execution: Some(execution),
        } => Err(format!(
            "context target execution is not active: {execution_id} ({:?})",
            execution.state
        )),
        ExecutionResponse::ExecutionLookup { execution: None } => {
            Err(format!("unknown context target execution: {execution_id}"))
        }
        other => Err(format!("unexpected execution lookup response: {other:?}")),
    }
}

fn project_context(
    context: &ContextPluginContext<'_, '_>,
    execution_id: String,
) -> Result<ExecutionContextProjection, String> {
    validate_identity("execution id", &execution_id)?;
    let injections =
        decode_injections(read_raw(context, &injections_key(&execution_id))?.as_deref())?;
    let entries = injections
        .into_iter()
        .map(|injection| {
            let resource = read_resource(
                context,
                &injection.source.resource_id,
                &injection.source.revision,
            )?
            .ok_or_else(|| {
                format!(
                    "missing durable context revision: {}@{}",
                    injection.source.resource_id, injection.source.revision
                )
            })?;
            Ok(ProjectedContextEntry {
                injection,
                resource,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ExecutionContextProjection {
        execution_id,
        entries,
    })
}

fn prepare_invocation(
    context: &ContextPluginContext<'_, '_>,
    state: &ContextStateService,
    execution_id: String,
    input: Bytes,
) -> Result<ContextInvocationPreparation, String> {
    require_active_execution(context, &execution_id)?;
    let projection = project_context(context, execution_id.clone())?;
    Ok(invocation_preparation(
        &projection,
        state.projection(&execution_id),
        state.projection_revision(&execution_id),
        &input,
    ))
}

fn invocation_preparation(
    projection: &ExecutionContextProjection,
    committed: Option<&ContextProjectionState>,
    projection_revision: ProjectionRevision,
    input: &Bytes,
) -> ContextInvocationPreparation {
    let mut candidates = assemble_prompt(projection)
        .sections
        .into_iter()
        .map(context_candidate)
        .filter(|candidate| {
            !committed
                .and_then(|state| state.admitted.get(&candidate.id))
                .is_some_and(is_reduced_item)
        })
        .collect::<Vec<_>>();

    if let Some(checkpoint) = committed
        .filter(|state| {
            state
                .admitted
                .values()
                .any(|item| item.retention == ContextRetention::Compact)
        })
        .and_then(|state| state.committed_checkpoint.as_ref())
    {
        candidates.push(checkpoint_candidate(checkpoint));
    }

    ContextInvocationPreparation {
        request_input_tokens: conservative_token_estimate(input.as_ref()),
        candidates,
        projection: projection_revision,
    }
}

fn materialize_invocation(
    context: &ContextPluginContext<'_, '_>,
    state: &ContextStateService,
    execution_id: String,
    input: Bytes,
    expected_projection: ProjectionRevision,
) -> Result<ContextInvocationMaterialization, String> {
    require_active_execution(context, &execution_id)?;
    let committed = state
        .projection(&execution_id)
        .ok_or_else(|| format!("context projection is not admitted: {execution_id}"))?;
    let projection = project_context(context, execution_id)?;
    let assembly = assemble_prompt(&projection);
    crate::materialization::materialize_invocation(
        &assembly,
        committed,
        input,
        &expected_projection,
    )
}

fn context_candidate(section: PromptSection) -> ContextCandidate {
    let mandatory = matches!(
        section.kind,
        PromptSectionKind::HarnessIdentity
            | PromptSectionKind::ProjectInstruction
            | PromptSectionKind::Skill
    );
    let cache = if mandatory {
        CachePlacement::StablePrefix
    } else {
        CachePlacement::Epoch
    };
    let retention = if mandatory {
        ContextRetention::Pinned
    } else {
        ContextRetention::Full
    };
    let content_identity = content_hash(section.content.as_ref()).as_str().to_owned();
    let (id, source, recovery) = match section.reference {
        Some(reference) => (
            format!("{}@{}", reference.resource_id, reference.revision),
            ContextSource::Exact {
                reference: reference.clone(),
            },
            Some(reference),
        ),
        None => (
            "phenix:harness-identity".to_owned(),
            ContextSource::Inline {
                identity: "phenix:harness-identity".to_owned(),
            },
            None,
        ),
    };
    ContextCandidate {
        id,
        source,
        content_identity,
        estimated_tokens: conservative_token_estimate(section.content.as_ref()),
        content: section.content,
        mandatory,
        retention,
        cache,
        recovery,
    }
}

fn checkpoint_candidate(checkpoint: &ProjectionCheckpoint) -> ContextCandidate {
    let identity = checkpoint_candidate_id(checkpoint);
    ContextCandidate {
        id: identity.clone(),
        source: ContextSource::Inline { identity },
        content_identity: checkpoint.content_identity.clone(),
        content: checkpoint.compact_view.clone(),
        estimated_tokens: conservative_token_estimate(checkpoint.compact_view.as_ref()),
        mandatory: false,
        retention: ContextRetention::DropAllowed,
        cache: CachePlacement::Epoch,
        recovery: None,
    }
}

fn checkpoint_candidate_id(checkpoint: &ProjectionCheckpoint) -> String {
    format!("phenix:checkpoint:{}", checkpoint.checkpoint_id)
}

fn is_reduced_item(item: &AdmittedContextItem) -> bool {
    item.retention == ContextRetention::Compact || item.form != ContextProjectionForm::Full
}

fn conservative_token_estimate(content: &[u8]) -> u64 {
    u64::try_from(content.len()).unwrap_or(u64::MAX)
}

fn read_resource(
    context: &ContextPluginContext<'_, '_>,
    resource_id: &ContextResourceId,
    revision: &ContextRevisionId,
) -> Result<Option<ContextResourceRevision>, String> {
    read_raw(context, &resource_key(resource_id, revision))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn list_descriptors(
    context: &ContextPluginContext<'_, '_>,
) -> Result<Vec<ContextDescriptor>, String> {
    let refs = decode_refs(read_raw(context, ALL_RESOURCES_KEY)?.as_deref())?;
    refs.into_iter()
        .map(|reference| {
            read_resource(context, &reference.resource_id, &reference.revision)?
                .map(|resource| resource.descriptor)
                .ok_or_else(|| {
                    format!(
                        "missing durable context revision: {}@{}",
                        reference.resource_id, reference.revision
                    )
                })
        })
        .collect()
}

fn read_raw(context: &ContextPluginContext<'_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&context_namespace(), key)
        .map_err(|error| error.to_string())
}

fn decode_refs(value: Option<&[u8]>) -> Result<Vec<ExactContextReference>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn decode_injections(value: Option<&[u8]>) -> Result<Vec<ContextInjection>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn resource_key(resource_id: &ContextResourceId, revision: &ContextRevisionId) -> String {
    format!("resource/{resource_id}/{revision}")
}

fn injections_key(execution_id: &str) -> String {
    format!("injections/{execution_id}")
}

fn injection_admission_key(execution_id: &str, admission_id: &str) -> String {
    format!("injection-admission/{execution_id}/{admission_id}")
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn content_hash(content: &[u8]) -> ContextRevisionId {
    let digest = Sha256::digest(content);
    let value: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    ContextRevisionId::parse(value).expect("sha256 hex is a valid context revision")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

#[cfg(test)]
mod preparation_tests {
    use super::*;

    #[test]
    fn invocation_preparation_keeps_request_budget_separate_from_context_candidates() {
        let projection = ExecutionContextProjection {
            execution_id: "execution-1".into(),
            entries: Vec::new(),
        };
        let preparation = invocation_preparation(
            &projection,
            None,
            ProjectionRevision {
                revision: 3,
                cache_epoch: 2,
            },
            &Bytes::from(b"request".to_vec()),
        );

        assert_eq!(preparation.request_input_tokens, 7);
        assert_eq!(preparation.projection.revision, 3);
        assert_eq!(preparation.projection.cache_epoch, 2);
        assert_eq!(preparation.candidates.len(), 1);
        let identity = &preparation.candidates[0];
        assert!(identity.mandatory);
        assert_eq!(identity.retention, ContextRetention::Pinned);
        assert_eq!(identity.cache, CachePlacement::StablePrefix);
        assert!(matches!(identity.source, ContextSource::Inline { .. }));
    }
}
