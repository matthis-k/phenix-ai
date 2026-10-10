use crate::{
    PromptSection, PromptSectionKind, assemble_prompt, context_component_id,
    projection_state::ContextProjectionState,
    state_service::{CONTEXT_PROJECTION_STATE_KEY, ContextStateService},
};
use phenix_core::{
    Authority, Bytes, ComponentInterface, ContextResourceId, ContextRevisionId, DurableSchema,
    PermissionId, PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance,
    PluginManifest, ResourceNamespace, RuntimeTraceEvent, SdkClient, ServiceContribution,
    ServiceId, TransactionOp,
};
use phenix_sdk::{
    AdmittedContextItem, AttemptOutcome, CachePlacement, CompactionProposal,
    ContextAdmissionRequest, ContextCandidate, ContextCodeQueryRequest, ContextCommand,
    ContextDescriptor, ContextInjection, ContextInjectionLifetime, ContextInjectionRequester,
    ContextInterface, ContextInvocationMaterialization, ContextInvocationPreparation,
    ContextProjectionForm, ContextReducerCommand, ContextReducerInterface, ContextReducerProposal,
    ContextReducerRequest, ContextReducerResponse, ContextReducerStage, ContextResourceKind,
    ContextResourceRevision, ContextResponse, ContextRetention, ContextScope, ContextSource,
    ContinuationExportResult, ContinuationImportRequest, ContinuationProjectionRequest,
    ExactContextReference, ExecutionCommand, ExecutionContextProjection, ExecutionInterface,
    ExecutionResourceCommand, ExecutionResourceInterface, ExecutionResourceResponse,
    ExecutionResponse, ExecutionState, LanguageCommand, LanguageInterface, LanguageResponse,
    ProjectedContextEntry, ProjectionCheckpoint, ProjectionRevision, RepositoryContextSource,
    RetentionTransition, StepAttemptCommand, StepAttemptInterface, StepAttemptPhase,
    StepAttemptResponse, UsageAttemptKind, WorkerTaskState, assemble_continuation_candidates,
    build_continuation_packet, choose_cache_aware_compaction, context_service,
    derive_continuation_delta, project_continuation_import, select_continuation_export,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const CONTEXT_PLUGIN: &str = "phenix.context";
const CONTEXT_NAMESPACE: &str = "phenix.context.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const ALL_RESOURCES_KEY: &str = "resources/@all";

struct ContextSdk<'host, 'runtime> {
    execution: SdkClient<'host, 'runtime, ExecutionInterface>,
    resources: SdkClient<'host, 'runtime, ExecutionResourceInterface>,
    attempts: SdkClient<'host, 'runtime, StepAttemptInterface>,
    reducer: SdkClient<'host, 'runtime, ContextReducerInterface>,
    language: SdkClient<'host, 'runtime, LanguageInterface>,
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
            attempts: SdkClient::new(host, context_component_id()),
            reducer: SdkClient::new(host, context_component_id()),
            language: SdkClient::new(host, context_component_id()),
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
    context_factory_with_reducer_stages(BTreeSet::new())
}

#[must_use]
pub fn context_factory_with_reducer_stages(
    enabled_reducer_stages: BTreeSet<ContextReducerStage>,
) -> Box<dyn PluginInstance> {
    Box::new(ContextPlugin {
        state: ContextStateService::default(),
        enabled_reducer_stages,
    })
}

pub(crate) fn context_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(CONTEXT_NAMESPACE).expect("static namespace is valid")
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).expect("static capability is valid")
}

struct ContextPlugin {
    state: ContextStateService,
    enabled_reducer_stages: BTreeSet<ContextReducerStage>,
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
        match &command {
            ContextCommand::RequestReduction { request }
            | ContextCommand::CommitReduction { request, .. } => {
                require_reducer_stage_enabled(&self.enabled_reducer_stages, request.stage)?;
            }
            _ => {}
        }
        let trace = context_command_trace(&command);
        context
            .kernel
            .record_runtime_trace(RuntimeTraceEvent::PolicyStage {
                policy: "phenix.context".into(),
                stage: trace.stage.clone(),
                outcome: "started".into(),
                subject: trace.subject.clone(),
                revision: trace.revision.clone(),
                reason: trace.reason.clone(),
            });
        let response = match handle(&context, &mut self.state, command) {
            Ok(response) => {
                context
                    .kernel
                    .record_runtime_trace(RuntimeTraceEvent::PolicyStage {
                        policy: "phenix.context".into(),
                        stage: trace.stage,
                        outcome: "completed".into(),
                        subject: trace.subject,
                        revision: trace.revision,
                        reason: context_response_summary(&response).or(trace.reason),
                    });
                response
            }
            Err(error) => {
                context
                    .kernel
                    .record_runtime_trace(RuntimeTraceEvent::PolicyStage {
                        policy: "phenix.context".into(),
                        stage: trace.stage,
                        outcome: "failed".into(),
                        subject: trace.subject,
                        revision: trace.revision,
                        reason: Some(error.clone()),
                    });
                return Err(error);
            }
        };
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn require_reducer_stage_enabled(
    enabled: &BTreeSet<ContextReducerStage>,
    stage: ContextReducerStage,
) -> Result<(), String> {
    if enabled.contains(&stage) {
        Ok(())
    } else {
        Err(format!("context reducer stage {stage:?} is disabled"))
    }
}

struct ContextCommandTrace {
    stage: String,
    subject: Option<String>,
    revision: Option<String>,
    reason: Option<String>,
}

fn context_command_trace(command: &ContextCommand) -> ContextCommandTrace {
    let (stage, subject, revision, reason) = match command {
        ContextCommand::Register { resource_id, .. } => (
            "resource_registration",
            Some(resource_id.to_string()),
            None,
            None,
        ),
        ContextCommand::Get {
            resource_id,
            revision,
        } => (
            "resource_lookup",
            Some(resource_id.to_string()),
            Some(revision.to_string()),
            None,
        ),
        ContextCommand::List => ("resource_list", None, None, None),
        ContextCommand::DiscoverRepository {
            workspace_id,
            sources,
        }
        | ContextCommand::DiscoverProjectInstructions {
            workspace_id,
            sources,
        } => (
            "repository_discovery",
            Some(workspace_id.clone()),
            None,
            Some(format!("sources={}", sources.len())),
        ),
        ContextCommand::LoadCodeQuery { request } => (
            "code_query_load",
            Some(request.execution_id.clone()),
            None,
            Some(request.reason.clone()),
        ),
        ContextCommand::Load {
            execution_id,
            resource_id,
            revision,
            ..
        } => (
            "resource_load",
            Some(execution_id.clone()),
            Some(revision.to_string()),
            Some(format!("resource={resource_id}")),
        ),
        ContextCommand::LoadDelegatedResult { task_id } => {
            ("delegated_result_load", Some(task_id.clone()), None, None)
        }
        ContextCommand::AdmitDelegatedResult { task_id } => (
            "delegated_result_admission",
            Some(task_id.clone()),
            None,
            None,
        ),
        ContextCommand::LoadOnce {
            execution_id,
            resource_id,
            revision,
            ..
        } => (
            "resource_load_once",
            Some(execution_id.clone()),
            Some(revision.to_string()),
            Some(format!("resource={resource_id}")),
        ),
        ContextCommand::Project { execution_id } => {
            ("context_projection", Some(execution_id.clone()), None, None)
        }
        ContextCommand::PrepareInvocation { execution_id, .. } => (
            "invocation_preparation",
            Some(execution_id.clone()),
            None,
            None,
        ),
        ContextCommand::MaterializeInvocation {
            execution_id,
            expected_projection,
            ..
        } => (
            "invocation_materialization",
            Some(execution_id.clone()),
            Some(format!(
                "{}:{}",
                expected_projection.revision, expected_projection.cache_epoch
            )),
            None,
        ),
        ContextCommand::GetProjectionState { execution_id } => {
            ("projection_state", Some(execution_id.clone()), None, None)
        }
        ContextCommand::Admit { request } => (
            "context_admission",
            Some(request.execution_id.clone()),
            None,
            Some(format!("candidates={}", request.candidates.len())),
        ),
        ContextCommand::EvaluateCompactionCost { .. } => {
            ("compaction_cost_evaluation", None, None, None)
        }
        ContextCommand::RequestReduction { request } => (
            "context_reduction_request",
            Some(request.execution_id.clone()),
            Some(format!(
                "{}:{}",
                request.expected_projection.revision, request.expected_projection.cache_epoch
            )),
            Some(format!("stage={:?}", request.stage)),
        ),
        ContextCommand::CommitReduction { request, proposal } => (
            "context_reduction_commit",
            Some(request.execution_id.clone()),
            Some(format!(
                "{}:{}",
                request.expected_projection.revision, request.expected_projection.cache_epoch
            )),
            Some(format!(
                "stage={:?} reduction={}",
                request.stage, proposal.reduction_id
            )),
        ),
        ContextCommand::PrepareCompaction { proposal } => (
            "compaction_prepare",
            Some(proposal.execution_id.clone()),
            None,
            None,
        ),
        ContextCommand::CommitCompaction {
            execution_id,
            checkpoint_id,
        } => (
            "compaction_commit",
            Some(execution_id.clone()),
            None,
            Some(format!("checkpoint={checkpoint_id}")),
        ),
        ContextCommand::InvalidateProjection { execution_id } => (
            "projection_invalidation",
            Some(execution_id.clone()),
            None,
            None,
        ),
        ContextCommand::ExportContinuation { request } => (
            "continuation_export",
            Some(request.export.execution_id.clone()),
            None,
            None,
        ),
        ContextCommand::ProjectContinuationImport { request } => (
            "continuation_import_projection",
            Some(request.execution_id.clone()),
            None,
            None,
        ),
    };
    ContextCommandTrace {
        stage: stage.into(),
        subject,
        revision,
        reason,
    }
}

fn context_response_summary(response: &ContextResponse) -> Option<String> {
    match response {
        ContextResponse::Registered { resource } => Some(format!(
            "resource={} revision={}",
            resource.descriptor.resource_id, resource.descriptor.revision
        )),
        ContextResponse::Resources { descriptors }
        | ContextResponse::Discovered { descriptors } => {
            Some(format!("resources={}", descriptors.len()))
        }
        ContextResponse::Loaded { injection, .. } => Some(format!(
            "resource={} revision={}",
            injection.source.resource_id, injection.source.revision
        )),
        ContextResponse::CodeQueryLoaded {
            injection, result, ..
        } => Some(format!(
            "resource={} revision={} repository={} entities={} relations={} truncated={}",
            injection.source.resource_id,
            injection.source.revision,
            result.repository_id,
            result.entities.len(),
            result.relations.len(),
            result.coverage.truncated
        )),
        ContextResponse::Projection { projection } => {
            Some(format!("entries={}", projection.entries.len()))
        }
        ContextResponse::InvocationPrepared { preparation } => Some(format!(
            "candidates={} request_input_tokens={} projection={}:{}",
            preparation.candidates.len(),
            preparation.request_input_tokens,
            preparation.projection.revision,
            preparation.projection.cache_epoch
        )),
        ContextResponse::InvocationMaterialized { materialization } => Some(format!(
            "projection={}:{} cache_prefix_bytes={}",
            materialization.projection.revision,
            materialization.projection.cache_epoch,
            materialization.cache_prefix_bytes
        )),
        ContextResponse::ProjectionState { projection }
        | ContextResponse::ProjectionInvalidated { projection } => Some(format!(
            "projection={}:{}",
            projection.revision, projection.cache_epoch
        )),
        ContextResponse::Admission { result, projection } => Some(format!(
            "result={result:?} projection={}:{}",
            projection.revision, projection.cache_epoch
        )),
        ContextResponse::ReductionProposed {
            proposal,
            measurement,
        } => Some(format!(
            "reduction={} stage={:?} saved_bytes={}",
            proposal.reduction_id, proposal.stage, measurement.marginal_saved_bytes
        )),
        ContextResponse::ReductionCommitted {
            proposal,
            measurement,
            commit,
        } => Some(format!(
            "reduction={} stage={:?} saved_bytes={} projection={}:{}",
            proposal.reduction_id,
            proposal.stage,
            measurement.marginal_saved_bytes,
            commit.committed_projection.revision,
            commit.committed_projection.cache_epoch
        )),
        ContextResponse::CompactionPrepared {
            checkpoint_id,
            projection,
        } => Some(format!(
            "checkpoint={} projection={}:{}",
            checkpoint_id, projection.revision, projection.cache_epoch
        )),
        ContextResponse::CompactionCommitted { commit } => Some(format!(
            "checkpoint={} projection={}:{}",
            commit.proposal.checkpoint.checkpoint_id,
            commit.committed_projection.revision,
            commit.committed_projection.cache_epoch
        )),
        _ => None,
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
        ContextCommand::DiscoverProjectInstructions {
            workspace_id,
            sources,
        } => Ok(ContextResponse::Discovered {
            descriptors: discover_project_instructions(context, &workspace_id, sources)?,
        }),
        ContextCommand::LoadCodeQuery { request } => load_code_query(context, state, request),
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
        ContextCommand::AdmitDelegatedResult { task_id } => {
            admit_delegated_result(context, state, task_id)
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
        ContextCommand::RequestReduction { request } => {
            require_active_execution(context, &request.execution_id)?;
            request_reduction(context, state, request)
        }
        ContextCommand::CommitReduction { request, proposal } => {
            require_active_execution(context, &request.execution_id)?;
            commit_reduction(context, state, request, proposal)
        }
        ContextCommand::ExportContinuation { request } => {
            Ok(ContextResponse::ContinuationExported {
                result: export_continuation(context, state, request)?,
            })
        }
        ContextCommand::ProjectContinuationImport { request } => {
            Ok(ContextResponse::ContinuationImportProjected {
                projection: import_continuation(context, request)?,
            })
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

fn request_reduction(
    context: &ContextPluginContext<'_, '_>,
    state: &ContextStateService,
    request: ContextReducerRequest,
) -> Result<ContextResponse, String> {
    let actual_projection = state.projection_revision(&request.execution_id);
    if request.expected_projection != actual_projection {
        return Err(format!(
            "context reducer request is stale: expected {:?}, actual {:?}",
            request.expected_projection, actual_projection
        ));
    }
    let response: ContextReducerResponse = context
        .sdk
        .reducer
        .invoke_projected(&ContextReducerCommand::Reduce {
            request: request.clone(),
        })
        .map_err(|error| format!("context reducer unavailable or failed: {error}"))?;
    let ContextReducerResponse::Proposal { proposal } = response;
    let measurement = proposal
        .measure_against(&request, &actual_projection)
        .map_err(|error| format!("context reducer proposal rejected: {error:?}"))?;
    verify_reducer_helper_attempt(context, &request, &proposal)?;
    Ok(ContextResponse::ReductionProposed {
        proposal,
        measurement,
    })
}

fn commit_reduction(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    request: ContextReducerRequest,
    proposal: ContextReducerProposal,
) -> Result<ContextResponse, String> {
    let actual_projection = state.projection_revision(&request.execution_id);
    let measurement = proposal
        .measure_against(&request, &actual_projection)
        .map_err(|error| format!("context reducer proposal rejected at commit: {error:?}"))?;
    verify_reducer_helper_attempt(context, &request, &proposal)?;

    let compaction = reduction_compaction_proposal(state, &request, &proposal)?;
    let previous = read_raw(context, CONTEXT_PROJECTION_STATE_KEY)?;
    let mut next = state.clone();
    let commit = next
        .commit_compaction_proposal(compaction)
        .map_err(|error| format!("context reduction commit failed: {error:?}"))?;
    persist_state(context, &mut next, previous)?;
    *state = next;

    Ok(ContextResponse::ReductionCommitted {
        proposal,
        measurement,
        commit,
    })
}

fn reduction_compaction_proposal(
    state: &ContextStateService,
    request: &ContextReducerRequest,
    proposal: &ContextReducerProposal,
) -> Result<CompactionProposal, String> {
    if proposal.omitted_item_ids.is_empty() && proposal.summaries.is_empty() {
        return Err("context reducer proposal makes no projection change".into());
    }

    let projection = state.projection(&request.execution_id).ok_or_else(|| {
        format!(
            "context projection is not admitted: {}",
            request.execution_id
        )
    })?;
    if projection.revision != request.expected_projection {
        return Err(format!(
            "context reducer commit is stale: expected {:?}, actual {:?}",
            request.expected_projection, projection.revision
        ));
    }

    let next_cache_epoch = projection
        .revision
        .cache_epoch
        .checked_add(1)
        .ok_or_else(|| "context reducer cache epoch overflow".to_owned())?;
    let mut transitions = Vec::new();
    let mut exact_sources = projection
        .committed_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.exact_sources.clone())
        .unwrap_or_default();
    let mut tool_groups = projection
        .committed_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.tool_groups.clone())
        .unwrap_or_default();
    let mut compact_view = projection
        .committed_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.compact_view.as_ref().to_vec())
        .unwrap_or_default();

    for eligible in &request.eligible {
        let admitted = projection
            .admitted
            .get(&eligible.item_id)
            .ok_or_else(|| format!("context reducer item is not admitted: {}", eligible.item_id))?;
        let summary = proposal
            .summaries
            .iter()
            .find(|summary| summary.item_id == eligible.item_id);
        let changes_item =
            proposal.omitted_item_ids.contains(&eligible.item_id) || summary.is_some();
        if changes_item && is_reduced_item(admitted) {
            return Err(format!(
                "context reducer cannot rewrite already reduced item: {}",
                eligible.item_id
            ));
        }

        if proposal.omitted_item_ids.contains(&eligible.item_id) {
            let recovery = eligible.recovery.clone().ok_or_else(|| {
                format!(
                    "context reducer omitted item without recovery: {}",
                    eligible.item_id
                )
            })?;
            exact_sources.push(recovery.clone());
            transitions.push(RetentionTransition {
                item_id: eligible.item_id.clone(),
                from: admitted.retention,
                to: ContextRetention::Reference,
                recovery: Some(recovery),
            });
        } else if let Some(summary) = summary {
            let recovery = eligible.recovery.clone().ok_or_else(|| {
                format!(
                    "context reducer summarized item without recovery: {}",
                    eligible.item_id
                )
            })?;
            exact_sources.extend(summary.exact_sources.iter().cloned());
            if !compact_view.is_empty() {
                compact_view.extend_from_slice(b"\n\n");
            }
            compact_view.extend_from_slice(b"[");
            compact_view.extend_from_slice(eligible.item_id.as_bytes());
            compact_view.extend_from_slice(b"]\n");
            compact_view.extend_from_slice(summary.content.as_ref());
            transitions.push(RetentionTransition {
                item_id: eligible.item_id.clone(),
                from: admitted.retention,
                to: ContextRetention::Compact,
                recovery: Some(recovery),
            });
        }
    }

    exact_sources.sort();
    exact_sources.dedup();
    tool_groups.sort_by(|left, right| left.call_id.cmp(&right.call_id));
    tool_groups.dedup_by(|left, right| left.call_id == right.call_id);
    let content_identity = content_hash(&compact_view).as_str().to_owned();

    Ok(CompactionProposal {
        execution_id: request.execution_id.clone(),
        expected_projection: request.expected_projection.clone(),
        next_cache_epoch,
        transitions,
        checkpoint: ProjectionCheckpoint {
            checkpoint_id: proposal.reduction_id.clone(),
            execution_id: request.execution_id.clone(),
            source_revision: request.expected_projection.clone(),
            content_identity,
            compact_view: Bytes::from(compact_view),
            exact_sources,
            tool_groups,
        },
    })
}

fn verify_reducer_helper_attempt(
    context: &ContextPluginContext<'_, '_>,
    request: &ContextReducerRequest,
    proposal: &phenix_sdk::ContextReducerProposal,
) -> Result<(), String> {
    let response: StepAttemptResponse = context
        .sdk
        .attempts
        .invoke_projected(&StepAttemptCommand::Get {
            attempt_id: proposal.helper_attempt_id.clone(),
        })
        .map_err(|error| format!("context reducer helper attempt lookup failed: {error}"))?;
    let StepAttemptResponse::AttemptLookup {
        attempt: Some(attempt),
    } = response
    else {
        return Err(format!(
            "context reducer proposal references unknown helper attempt: {}",
            proposal.helper_attempt_id
        ));
    };

    if attempt.attribution.execution_id != request.execution_id {
        return Err("context reducer helper attempt belongs to another execution".into());
    }
    if attempt.attribution.kind != UsageAttemptKind::Helper {
        return Err("context reducer helper attempt is not an ordinary helper invocation".into());
    }
    if attempt.attribution.parent_attempt_id.as_deref() != Some(request.parent_attempt_id.as_str())
    {
        return Err("context reducer helper attempt parent does not match request".into());
    }
    if attempt.reservation_id.is_none() {
        return Err("context reducer helper attempt has no charged reservation".into());
    }
    if attempt.phase != StepAttemptPhase::Settled
        || attempt.outcome != Some(AttemptOutcome::Succeeded)
    {
        return Err("context reducer helper attempt did not settle successfully".into());
    }
    Ok(())
}

fn import_continuation(
    context: &ContextPluginContext<'_, '_>,
    request: ContinuationImportRequest,
) -> Result<phenix_sdk::ContinuationImportProjection, String> {
    require_active_execution(context, &request.execution_id)?;
    project_continuation_import(&request)
        .map_err(|error| format!("continuation import rejected: {error:?}"))
}

fn export_continuation(
    context: &ContextPluginContext<'_, '_>,
    state: &ContextStateService,
    request: ContinuationProjectionRequest,
) -> Result<ContinuationExportResult, String> {
    require_active_execution(context, &request.export.execution_id)?;
    let projection = state.projection_revision(&request.export.execution_id);
    let observed_snapshot = format!(
        "{}:{}:{}:{}",
        request.export.execution_id,
        request.export.checkpoint_id,
        projection.revision,
        projection.cache_epoch
    );
    if request.expected_source_snapshot != observed_snapshot {
        return Ok(ContinuationExportResult::StaleSnapshot {
            expected: request.expected_source_snapshot,
            observed: observed_snapshot,
        });
    }

    let mut candidates = assemble_continuation_candidates(&request.source_state)
        .map_err(|error| format!("invalid continuation source state: {error:?}"))?;
    candidates.extend(request.candidates.clone());
    let built = build_continuation_packet(&request.export, &observed_snapshot, &candidates)?;
    let ContinuationExportResult::Packet { packet } = built else {
        return Ok(built);
    };
    let delta = request
        .base_packet
        .as_ref()
        .map(|base| derive_continuation_delta(base, &packet))
        .transpose()
        .map_err(|error| format!("cannot derive continuation delta: {error:?}"))?;
    select_continuation_export(
        &request.export,
        packet,
        delta,
        request.acknowledged_base_digest.as_deref(),
        request.measurements,
    )
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
    sources: Vec<RepositoryContextSource>,
) -> Result<Vec<ContextDescriptor>, String> {
    discover_repository_sources(context, workspace_id, sources, None)
}

fn discover_project_instructions(
    context: &ContextPluginContext<'_, '_>,
    workspace_id: &str,
    sources: Vec<RepositoryContextSource>,
) -> Result<Vec<ContextDescriptor>, String> {
    discover_repository_sources(
        context,
        workspace_id,
        sources,
        Some(ContextResourceKind::ProjectInstruction),
    )
}

fn discover_repository_sources(
    context: &ContextPluginContext<'_, '_>,
    workspace_id: &str,
    mut sources: Vec<RepositoryContextSource>,
    forced_kind: Option<ContextResourceKind>,
) -> Result<Vec<ContextDescriptor>, String> {
    validate_identity("workspace id", workspace_id)?;
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    let mut descriptors = Vec::new();
    for source in sources {
        let kind = match forced_kind
            .clone()
            .or_else(|| project_file_kind(&source.path))
        {
            Some(kind) => kind,
            None => continue,
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

fn load_code_query(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    request: ContextCodeQueryRequest,
) -> Result<ContextResponse, String> {
    require_active_execution(context, &request.execution_id)?;

    let query = request.query;
    let response: LanguageResponse = context
        .sdk
        .language
        .invoke_projected(&LanguageCommand::Query {
            query: query.clone(),
        })
        .map_err(|error| format!("semantic code query unavailable: {error}"))?;
    let LanguageResponse::Query { result } = response else {
        return Err(format!(
            "semantic code query returned unexpected response: {response:?}"
        ));
    };

    let content = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
    let identity_material = serde_json::to_vec(&(request.scope.clone(), query, &result))
        .map_err(|error| error.to_string())?;
    let identity = content_hash(&identity_material);
    let resource_id = ContextResourceId::parse(format!("code-query:{}", identity.as_str()))
        .map_err(str::to_owned)?;
    let resource = register_resource(
        context,
        resource_id,
        ContextResourceKind::External,
        format!(
            "language:code-query:{}@{}",
            result.repository_id, result.coverage.repository_sequence
        ),
        request.scope,
        Bytes::from(content),
    )?;
    let (injection, resource) = load_context(
        context,
        state,
        request.execution_id,
        resource.descriptor.resource_id.clone(),
        resource.descriptor.revision.clone(),
        request.requester,
        request.lifetime,
        request.reason,
    )?;

    Ok(ContextResponse::CodeQueryLoaded {
        injection,
        resource,
        result,
    })
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
    let ExecutionResourceResponse::DelegatedTaskLookup { task: Some(record) } = response else {
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

fn admit_delegated_result(
    context: &ContextPluginContext<'_, '_>,
    state: &mut ContextStateService,
    task_id: String,
) -> Result<ContextResponse, String> {
    let (injection, resource) = load_delegated_result(context, state, task_id.clone())?;
    let response: ExecutionResourceResponse = context
        .sdk
        .resources
        .invoke_projected(&ExecutionResourceCommand::GetDelegated {
            task_id: task_id.clone(),
        })
        .map_err(|error| format!("delegated task lookup failed: {error}"))?;
    let ExecutionResourceResponse::DelegatedTaskLookup { task: Some(record) } = response else {
        return Err(format!(
            "unknown delegated task after result load: {task_id}"
        ));
    };
    let parent_plan = record
        .binding
        .parent_plan
        .clone()
        .ok_or_else(|| format!("delegated task has no parent plan: {task_id}"))?;
    let parent_execution = record.task.parent_execution;
    let preparation = prepare_invocation(
        context,
        state,
        parent_execution.clone(),
        Bytes::from(Vec::new()),
    )?;
    let admitted = handle_state_command(
        context,
        state,
        ContextCommand::Admit {
            request: ContextAdmissionRequest {
                execution_id: parent_execution,
                step_plan: parent_plan,
                candidates: preparation.candidates,
                cache_epoch: preparation.projection.cache_epoch,
            },
        },
    )?;
    let ContextResponse::Admission { result, projection } = admitted else {
        return Err("context state service returned a non-admission response".into());
    };
    Ok(ContextResponse::DelegatedResultAdmitted {
        injection,
        resource,
        result,
        projection,
    })
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
    if let Some(receipt_key) = receipt.as_deref()
        && let Some(existing) = read_raw(context, receipt_key)?
    {
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
    fn reducer_stages_are_independently_disableable() {
        let mut enabled = BTreeSet::new();
        enabled.insert(ContextReducerStage::CodeEvidence);

        assert!(require_reducer_stage_enabled(&enabled, ContextReducerStage::CodeEvidence).is_ok());
        assert_eq!(
            require_reducer_stage_enabled(&enabled, ContextReducerStage::ObservationSummary),
            Err("context reducer stage ObservationSummary is disabled".into())
        );
        assert_eq!(
            require_reducer_stage_enabled(&BTreeSet::new(), ContextReducerStage::CodeEvidence),
            Err("context reducer stage CodeEvidence is disabled".into())
        );
    }

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
