use crate::{
    error::{MemoryError, MemoryResult},
    freshness::{
        deterministic_outcome, exact_support_is_current, initial_state, observe_revision_change,
    },
    persistence::{
        insert_record, insert_record_with_sidecar_secondary_and_updates, load_records,
        load_secondary_ids, load_secondary_ids_page, read_record, write_record,
        write_record_with_secondary_entry,
    },
    retrieval,
};
use phenix_core::{
    Authority, Bytes, CallableId, CapabilityId, ComponentInterface, DurableSchema, EventTypeId,
    PluginContext, PluginExecution, PluginHost, PluginId, PluginInstance, PluginManifest,
    ResourceNamespace, RoutingProfileId, SdkClient, ServiceContribution, ServiceId,
};
use phenix_sdk::{
    context_compaction_service, context_expansion_service, memory_consolidate_callable,
    memory_extract_callable, memory_resolve_callable, memory_service, memory_summarize_callable,
    memory_validate_callable, ContextCheckpoint, ContextCompactionCommand,
    ContextCompactionInterface, ContextCompactionRequest, ContextCompactionResponse,
    HelperInvocationCommand, HelperInvocationInterface, HelperInvocationKind,
    HelperInvocationRequest, HelperInvocationResponse, MemoryCanonicalReference, MemoryCommand,
    MemoryConsolidationRequest, MemoryDependencyRevision, MemoryEmbeddingInterface,
    MemoryEmbeddingRequest, MemoryEmbeddingResponse, MemoryExpansion, MemoryExtractionRequest,
    MemoryFreshness, MemoryFreshnessRecord, MemoryInterface, MemoryKind, MemoryNode,
    MemoryRankCandidate, MemoryRankInterface, MemoryRankRequest, MemoryRankResponse,
    MemoryRecallQuery, MemoryRecord, MemoryResponse, MemoryRevalidationOutcome,
    MemoryRevisionCursor, MemoryScope, MemorySourceReference,
};

const MEMORY_PLUGIN: &str = "phenix.memory";
const MEMORY_NAMESPACE: &str = "phenix.memory.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const RECORD_INDEX: &str = "index/records";
const DEPENDENCY_INDEX: &str = "index/dependencies";
const NODE_INDEX: &str = "index/nodes";
const CHECKPOINT_INDEX: &str = "index/checkpoints";
pub(crate) const RECALL_EVENT: &str = "phenix.memory.recall";
pub(crate) const COMPACTION_EVENT: &str = "phenix.memory.compaction";
pub(crate) const REVALIDATION_EVENT: &str = "phenix.memory.revalidation";

pub(crate) struct MemorySdk<'host, 'runtime> {
    invocation: SdkClient<'host, 'runtime, HelperInvocationInterface>,
    embed: SdkClient<'host, 'runtime, MemoryEmbeddingInterface>,
    rank: SdkClient<'host, 'runtime, MemoryRankInterface>,
}

pub(crate) type MemoryContext<'host, 'runtime> =
    PluginContext<'host, 'runtime, MemorySdk<'host, 'runtime>>;

#[must_use]
pub fn memory_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(MEMORY_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: [
            memory_service(),
            context_compaction_service(),
            context_expansion_service(),
        ]
        .into_iter()
        .map(|service| ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service,
            priority: 100,
            required_authority: Authority::default(),
        })
        .collect(),
        resource_namespaces: vec![memory_namespace()],
        maximum_authority: persistence_authority(),
    }
}

#[must_use]
pub fn memory_factory() -> Box<dyn PluginInstance> {
    Box::new(MemoryPlugin)
}

pub(crate) fn memory_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(MEMORY_NAMESPACE).expect("static namespace is valid")
}

pub(crate) fn persistence_authority() -> Authority {
    Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ])
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

fn observe(context: &MemoryContext<'_, '_>, event: &str, payload: serde_json::Value) {
    let event_type = EventTypeId::parse(event).expect("static memory event type is valid");
    let Ok(payload) = serde_json::to_vec(&payload) else {
        return;
    };
    let _ = context.kernel.dispatch_event(event_type, 1, 0, 0, payload);
}

fn context<'host, 'runtime>(host: &'host PluginHost<'runtime>) -> MemoryContext<'host, 'runtime> {
    PluginContext::new(
        host,
        MemorySdk {
            invocation: SdkClient::new(host, crate::memory_component_id()),
            embed: SdkClient::new(host, crate::memory_component_id()),
            rank: SdkClient::new(host, crate::memory_component_id()),
        },
        (),
        (),
    )
}

struct MemoryPlugin;

impl PluginInstance for MemoryPlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host)
            .kernel
            .register_durable_schema(&DurableSchema::new(memory_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        let context = context(host);
        if service == &memory_service() {
            let interface = MemoryInterface::interface_id();
            let command = context
                .kernel
                .decode_projected::<MemoryCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response = handle(&context, command).map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &context_compaction_service() {
            let interface = ContextCompactionInterface::interface_id();
            let command = context
                .kernel
                .decode_projected::<ContextCompactionCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response =
                handle_compaction(&context, command).map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        if service == &context_expansion_service() {
            let interface = phenix_sdk::ContextExpansionInterface::interface_id();
            let command = context
                .kernel
                .decode_projected::<phenix_sdk::ContextExpansionCommand>(&interface, input)
                .map_err(|error| error.to_string())?;
            let response =
                handle_expansion(&context, command).map_err(|error| error.to_string())?;
            return context
                .kernel
                .encode_value(&response)
                .map_err(|error| error.to_string());
        }
        Err(format!("unsupported memory service: {service}"))
    }
}

fn handle(context: &MemoryContext<'_, '_>, command: MemoryCommand) -> MemoryResult<MemoryResponse> {
    match command {
        MemoryCommand::Record { record } => Ok(MemoryResponse::Record {
            record: record_memory(context, record)?,
        }),
        MemoryCommand::RecordNode { node } => Ok(MemoryResponse::Node {
            node: Some(record_node(context, node)?),
        }),
        MemoryCommand::Get { id } => Ok(MemoryResponse::Memory {
            record: read_record(context, &record_key(&MemoryKey::parse(id)?))?,
        }),
        MemoryCommand::GetFreshness { id } => Ok(MemoryResponse::Freshness {
            state: get_freshness(context, id)?,
        }),
        MemoryCommand::GetNode { id } => Ok(MemoryResponse::Node {
            node: read_record(context, &node_key(&MemoryKey::parse(id)?))?,
        }),
        MemoryCommand::Recall { query } => {
            let records = recall_memory(context, query)?;
            observe(
                context,
                RECALL_EVENT,
                serde_json::json!({ "records": records.len() }),
            );
            Ok(MemoryResponse::Recall { records })
        }
        MemoryCommand::Extract { request } => Ok(MemoryResponse::Record {
            record: extract_memory(context, request)?,
        }),
        MemoryCommand::Consolidate { request } => Ok(MemoryResponse::Record {
            record: consolidate_memory(context, request)?,
        }),
        MemoryCommand::ObserveRevision {
            service,
            resource,
            revision,
            observed_at,
            limit,
        } => Ok(MemoryResponse::Affected {
            memory_ids: observe_revision(context, service, resource, revision, observed_at, limit)?,
        }),
        MemoryCommand::ObserveRevisionPage {
            service,
            resource,
            revision,
            observed_at,
            limit,
            cursor,
        } => {
            let (memory_ids, next_cursor) = observe_revision_page(
                context,
                service,
                resource,
                revision,
                observed_at,
                limit,
                cursor,
            )?;
            Ok(MemoryResponse::AffectedPage {
                memory_ids,
                next_cursor,
            })
        }
        MemoryCommand::ObserveConflict {
            source,
            affected_ids,
            observed_at,
        } => Ok(MemoryResponse::Affected {
            memory_ids: observe_conflict(context, source, affected_ids, observed_at)?,
        }),
        MemoryCommand::BindCanonicalReference {
            id,
            reference,
            observed_at,
        } => Ok(MemoryResponse::Freshness {
            state: Some(bind_canonical_reference(
                context,
                id,
                reference,
                observed_at,
            )?),
        }),
        MemoryCommand::Revalidate {
            id,
            execution_id,
            parent_attempt_id,
            profile_id,
            at,
        } => {
            let state = revalidate_memory(
                context,
                id.clone(),
                execution_id,
                parent_attempt_id,
                profile_id,
                at,
            )?;
            observe(
                context,
                REVALIDATION_EVENT,
                serde_json::json!({ "memory_id": id, "at": at }),
            );
            Ok(MemoryResponse::Freshness { state: Some(state) })
        }
        MemoryCommand::ExpandNode { id } => Ok(MemoryResponse::Expansion {
            expansion: expand_node(context, &MemoryKey::parse(id)?)?,
        }),
        MemoryCommand::Promote {
            id,
            promoted_id,
            scope,
            created_at,
        } => Ok(MemoryResponse::Record {
            record: promote_memory(context, id, promoted_id, scope, created_at)?,
        }),
    }
}

fn extract_memory(
    context: &MemoryContext<'_, '_>,
    request: MemoryExtractionRequest,
) -> MemoryResult<MemoryRecord> {
    if request.observations.is_empty() {
        return Err(MemoryError::Invalid(
            "memory extraction requires at least one observation".into(),
        ));
    }
    let mut source_refs = Vec::new();
    let mut supporting_dependencies = Vec::new();
    for observation in &request.observations {
        validate_text("memory extraction observation", &observation.content)?;
        source_refs.extend(observation.source_refs.iter().cloned());
        supporting_dependencies.extend(observation.supporting_dependencies.iter().cloned());
    }
    normalize_sources(&mut source_refs)?;
    normalize_supporting_dependencies(&mut supporting_dependencies)?;
    if source_refs.is_empty() {
        return Err(MemoryError::Invalid(
            "memory extraction requires exact durable provenance".into(),
        ));
    }
    let input = serde_json::to_vec(&request.observations)
        .map_err(|error| MemoryError::Provider(error.to_string()))?;
    let content = routed_memory_text(
        context,
        &request.execution_id,
        &request.parent_attempt_id,
        &request.profile_id,
        memory_extract_callable(),
        input,
        "memory extraction",
    )?;
    record_memory(
        context,
        MemoryRecord {
            id: request.id,
            kind: request.kind,
            scope: request.scope,
            content,
            source_refs,
            supporting_dependencies,
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            created_at: request.created_at,
        },
    )
}

fn consolidate_memory(
    context: &MemoryContext<'_, '_>,
    request: MemoryConsolidationRequest,
) -> MemoryResult<MemoryRecord> {
    let MemoryConsolidationRequest {
        execution_id,
        parent_attempt_id,
        profile_id,
        mut ids,
        consolidated_id,
        created_at,
    } = request;
    if !(2..=100).contains(&ids.len()) {
        return Err(MemoryError::Invalid(
            "memory consolidation requires between 2 and 100 records".into(),
        ));
    }
    ids.sort();
    let original_len = ids.len();
    ids.dedup();
    if ids.len() != original_len {
        return Err(MemoryError::Invalid(
            "memory consolidation requires distinct records".into(),
        ));
    }

    let mut records = Vec::with_capacity(ids.len());
    for id in &ids {
        let key = MemoryKey::parse(id.clone())?;
        let record: MemoryRecord = read_record(context, &record_key(&key))?
            .ok_or_else(|| MemoryError::Missing(format!("memory {id}")))?;
        records.push(record);
    }
    let first = &records[0];
    if records
        .iter()
        .any(|record| record.kind != first.kind || record.scope != first.scope)
    {
        return Err(MemoryError::Invalid(
            "consolidated memory must have the same kind and scope".into(),
        ));
    }
    let mut source_refs = records
        .iter()
        .flat_map(|record| record.source_refs.iter().cloned())
        .collect::<Vec<_>>();
    normalize_sources(&mut source_refs)?;
    let mut supporting_dependencies = records
        .iter()
        .flat_map(|record| record.supporting_dependencies.iter().cloned())
        .collect::<Vec<_>>();
    normalize_supporting_dependencies(&mut supporting_dependencies)?;
    let input =
        serde_json::to_vec(&records).map_err(|error| MemoryError::Provider(error.to_string()))?;
    let content = routed_memory_text(
        context,
        &execution_id,
        &parent_attempt_id,
        &profile_id,
        memory_consolidate_callable(),
        input,
        "memory consolidation",
    )?;
    record_memory(
        context,
        MemoryRecord {
            id: consolidated_id,
            kind: first.kind,
            scope: first.scope.clone(),
            content,
            source_refs,
            supporting_dependencies,
            supersedes: ids,
            valid_from: None,
            valid_until: None,
            created_at,
        },
    )
}

fn get_freshness(
    context: &MemoryContext<'_, '_>,
    id: String,
) -> MemoryResult<Option<MemoryFreshnessRecord>> {
    let id = MemoryKey::parse(id)?;
    let Some(record) = read_record::<MemoryRecord>(context, &record_key(&id))? else {
        return Ok(None);
    };
    Ok(Some(
        read_record(context, &freshness_key(&id))?.unwrap_or_else(|| initial_state(&record, None)),
    ))
}

fn bind_canonical_reference(
    context: &MemoryContext<'_, '_>,
    id: String,
    reference: MemoryCanonicalReference,
    observed_at: u64,
) -> MemoryResult<MemoryFreshnessRecord> {
    validate_text("canonical resource", &reference.resource)?;
    if reference.service == memory_service() {
        return Err(MemoryError::Invalid(
            "decision memory cannot use memory as its canonical decision service".into(),
        ));
    }
    let id = MemoryKey::parse(id)?;
    let record: MemoryRecord = read_record(context, &record_key(&id))?
        .ok_or_else(|| MemoryError::Missing(format!("memory {}", id.as_str())))?;
    if record.kind != MemoryKind::Decision {
        return Err(MemoryError::Invalid(
            "canonical references are only valid for decision memory".into(),
        ));
    }
    let state_key = freshness_key(&id);
    let mut state: MemoryFreshnessRecord =
        read_record(context, &state_key)?.unwrap_or_else(|| initial_state(&record, None));
    let entry = dependency_key(&reference.service, &reference.resource);
    state.canonical_reference = Some(reference);
    state.changed_at = observed_at;
    write_record_with_secondary_entry(
        context,
        &state_key,
        &state,
        DEPENDENCY_INDEX,
        &entry,
        id.as_str(),
    )?;
    Ok(state)
}

fn observe_revision(
    context: &MemoryContext<'_, '_>,
    service: ServiceId,
    resource: String,
    revision: String,
    observed_at: u64,
    limit: u32,
) -> MemoryResult<Vec<String>> {
    validate_text("source resource", &resource)?;
    validate_text("source revision", &revision)?;
    if !(1..=100).contains(&limit) {
        return Err(MemoryError::Invalid(
            "revision observation limit must be between 1 and 100".into(),
        ));
    }

    let entry = dependency_key(&service, &resource);
    let ids = load_secondary_ids(context, DEPENDENCY_INDEX, &entry, limit as usize)?;
    observe_revision_ids(context, &service, &resource, &revision, observed_at, ids)
}

fn observe_revision_page(
    context: &MemoryContext<'_, '_>,
    service: ServiceId,
    resource: String,
    revision: String,
    observed_at: u64,
    limit: u32,
    cursor: Option<MemoryRevisionCursor>,
) -> MemoryResult<(Vec<String>, Option<MemoryRevisionCursor>)> {
    validate_text("source resource", &resource)?;
    validate_text("source revision", &revision)?;
    if !(1..=100).contains(&limit) {
        return Err(MemoryError::Invalid(
            "revision observation limit must be between 1 and 100".into(),
        ));
    }
    if let Some(cursor) = cursor.as_ref() {
        if cursor.service != service || cursor.resource != resource || cursor.revision != revision {
            return Err(MemoryError::Invalid(
                "revision observation cursor does not match dependency revision".into(),
            ));
        }
        validate_text(
            "revision observation cursor memory id",
            &cursor.after_memory_id,
        )?;
    }

    let entry = dependency_key(&service, &resource);
    let (ids, has_more) = load_secondary_ids_page(
        context,
        DEPENDENCY_INDEX,
        &entry,
        cursor
            .as_ref()
            .map(|cursor| cursor.after_memory_id.as_str()),
        limit as usize,
    )?;
    let page_last_id = ids.last().cloned();
    let affected = observe_revision_ids(context, &service, &resource, &revision, observed_at, ids)?;
    let next_cursor = if has_more {
        page_last_id.map(|after_memory_id| MemoryRevisionCursor {
            service,
            resource,
            revision,
            after_memory_id,
        })
    } else {
        None
    };
    Ok((affected, next_cursor))
}

fn observe_revision_ids(
    context: &MemoryContext<'_, '_>,
    service: &ServiceId,
    resource: &str,
    revision: &str,
    observed_at: u64,
    ids: Vec<String>,
) -> MemoryResult<Vec<String>> {
    let mut affected = Vec::new();
    for id in ids {
        let key = MemoryKey::parse(id.clone())?;
        let record: MemoryRecord = read_record(context, &record_key(&key))?.ok_or_else(|| {
            MemoryError::Persistence(format!("missing dependency-indexed memory {id}"))
        })?;
        let state_key = freshness_key(&key);
        let mut state: MemoryFreshnessRecord =
            read_record(context, &state_key)?.unwrap_or_else(|| initial_state(&record, None));
        if observe_revision_change(&mut state, service, resource, revision, observed_at) {
            write_record(context, &state_key, &state)?;
            affected.push(id);
        }
    }
    Ok(affected)
}

fn observe_conflict(
    context: &MemoryContext<'_, '_>,
    source: MemorySourceReference,
    mut affected_ids: Vec<String>,
    observed_at: u64,
) -> MemoryResult<Vec<String>> {
    if affected_ids.is_empty() || affected_ids.len() > 100 {
        return Err(MemoryError::Invalid(
            "conflicting evidence must affect between 1 and 100 memories".into(),
        ));
    }
    affected_ids.sort();
    let original_len = affected_ids.len();
    affected_ids.dedup();
    if affected_ids.len() != original_len {
        return Err(MemoryError::Invalid(
            "conflicting evidence requires distinct affected memories".into(),
        ));
    }

    let mut sources = vec![source];
    normalize_sources(&mut sources)?;
    let source = sources
        .pop()
        .expect("one validated conflicting evidence source remains");
    let dependency = MemoryDependencyRevision {
        service: source.service.clone(),
        resource: source.resource.clone(),
        revision: None,
    };
    let entry = dependency_key(&source.service, &source.resource);
    let mut affected = Vec::new();

    for id in affected_ids {
        let key = MemoryKey::parse(id.clone())?;
        let record: MemoryRecord = read_record(context, &record_key(&key))?
            .ok_or_else(|| MemoryError::Missing(format!("memory {id}")))?;
        let state_key = freshness_key(&key);
        let mut state: MemoryFreshnessRecord =
            read_record(context, &state_key)?.unwrap_or_else(|| initial_state(&record, None));
        if !state.dependencies.contains(&dependency) {
            state.dependencies.push(dependency.clone());
            state.dependencies.sort();
        }
        if state.freshness == MemoryFreshness::Current {
            state.freshness = MemoryFreshness::NeedsValidation;
            state.changed_at = observed_at;
            affected.push(id.clone());
        }
        write_record_with_secondary_entry(
            context,
            &state_key,
            &state,
            DEPENDENCY_INDEX,
            &entry,
            &id,
        )?;
    }
    Ok(affected)
}

fn revalidate_memory(
    context: &MemoryContext<'_, '_>,
    id: String,
    execution_id: String,
    parent_attempt_id: String,
    profile_id: RoutingProfileId,
    at: u64,
) -> MemoryResult<MemoryFreshnessRecord> {
    let id = MemoryKey::parse(id)?;
    let record: MemoryRecord = read_record(context, &record_key(&id))?
        .ok_or_else(|| MemoryError::Missing(format!("memory {}", id.as_str())))?;
    let state_key = freshness_key(&id);
    let mut state: MemoryFreshnessRecord =
        read_record(context, &state_key)?.unwrap_or_else(|| initial_state(&record, None));

    let outcome = match deterministic_outcome(&record, &state, at) {
        MemoryRevalidationOutcome::KeepCurrent => return Ok(state),
        MemoryRevalidationOutcome::RetainHistorical => return Ok(state),
        MemoryRevalidationOutcome::Expire => MemoryRevalidationOutcome::Expire,
        MemoryRevalidationOutcome::Supersede => MemoryRevalidationOutcome::Supersede,
        MemoryRevalidationOutcome::NeedsValidation => {
            if !exact_support_is_current(&record, &state) {
                MemoryRevalidationOutcome::NeedsValidation
            } else {
                let validated = routed_revalidation(
                    context,
                    &execution_id,
                    &parent_attempt_id,
                    &profile_id,
                    memory_validate_callable(),
                    &record,
                    &state,
                )?;
                if validated == MemoryRevalidationOutcome::NeedsValidation {
                    routed_revalidation(
                        context,
                        &execution_id,
                        &parent_attempt_id,
                        &profile_id,
                        memory_resolve_callable(),
                        &record,
                        &state,
                    )?
                } else {
                    validated
                }
            }
        }
    };

    match outcome {
        MemoryRevalidationOutcome::KeepCurrent if exact_support_is_current(&record, &state) => {
            state.freshness = MemoryFreshness::Current;
        }
        MemoryRevalidationOutcome::KeepCurrent | MemoryRevalidationOutcome::NeedsValidation => {
            state.freshness = MemoryFreshness::NeedsValidation;
        }
        MemoryRevalidationOutcome::Supersede
        | MemoryRevalidationOutcome::Expire
        | MemoryRevalidationOutcome::RetainHistorical => {
            state.freshness = MemoryFreshness::Historical;
        }
    }
    state.changed_at = at;
    write_record(context, &state_key, &state)?;
    Ok(state)
}

fn routed_revalidation(
    context: &MemoryContext<'_, '_>,
    execution_id: &str,
    parent_attempt_id: &str,
    profile_id: &RoutingProfileId,
    callable_id: CallableId,
    record: &MemoryRecord,
    state: &MemoryFreshnessRecord,
) -> MemoryResult<MemoryRevalidationOutcome> {
    let input = serde_json::to_vec(&(record, state))
        .map_err(|error| MemoryError::Provider(error.to_string()))?;
    let output = routed_model_bytes(
        context,
        HelperInvocationRequest {
            execution_id: execution_id.to_owned(),
            parent_attempt_id: parent_attempt_id.to_owned(),
            profile_id: profile_id.clone(),
            kind: HelperInvocationKind::Verification,
            callable_id,
            input: Bytes::new(input),
            tools: Vec::new(),
        },
        "memory revalidation",
    )?;
    serde_json::from_slice(&output).map_err(|error| {
        MemoryError::Provider(format!("invalid memory revalidation outcome: {error}"))
    })
}

fn routed_memory_text(
    context: &MemoryContext<'_, '_>,
    execution_id: &str,
    parent_attempt_id: &str,
    profile_id: &RoutingProfileId,
    callable_id: CallableId,
    input: Vec<u8>,
    label: &str,
) -> MemoryResult<String> {
    let output = routed_model_bytes(
        context,
        HelperInvocationRequest {
            execution_id: execution_id.to_owned(),
            parent_attempt_id: parent_attempt_id.to_owned(),
            profile_id: profile_id.clone(),
            kind: HelperInvocationKind::Helper,
            callable_id,
            input: Bytes::new(input),
            tools: Vec::new(),
        },
        label,
    )?;
    let text = String::from_utf8(output)
        .map_err(|error| MemoryError::Provider(error.to_string()))?
        .trim()
        .to_owned();
    validate_text(label, &text)?;
    Ok(text)
}

fn routed_model_bytes(
    context: &MemoryContext<'_, '_>,
    request: HelperInvocationRequest,
    label: &str,
) -> MemoryResult<Vec<u8>> {
    let response: HelperInvocationResponse = context
        .sdk
        .invocation
        .invoke_projected(&HelperInvocationCommand::Invoke { request })
        .map_err(|error| MemoryError::Provider(error.to_string()))?;
    if response.output.as_ref().is_empty() {
        return Err(MemoryError::Provider(format!(
            "{label} returned an empty model response"
        )));
    }
    Ok(response.output.as_ref().to_vec())
}

fn recall_memory(
    context: &MemoryContext<'_, '_>,
    query: MemoryRecallQuery,
) -> MemoryResult<Vec<MemoryRecord>> {
    let records: Vec<MemoryRecord> = load_records(context, RECORD_INDEX, record_key_str)?;
    let mut current = Vec::new();
    for record in records {
        let id = MemoryKey::parse(record.id.clone())?;
        let state_key = freshness_key(&id);
        let mut state: MemoryFreshnessRecord =
            read_record(context, &state_key)?.unwrap_or_else(|| initial_state(&record, None));
        match deterministic_outcome(&record, &state, query.at) {
            MemoryRevalidationOutcome::KeepCurrent => current.push(record),
            MemoryRevalidationOutcome::Expire => {
                if state.freshness == MemoryFreshness::Current {
                    state.freshness = MemoryFreshness::Historical;
                    state.changed_at = query.at;
                    write_record(context, &state_key, &state)?;
                }
                if query.at < state.changed_at {
                    current.push(record);
                }
            }
            MemoryRevalidationOutcome::NeedsValidation
            | MemoryRevalidationOutcome::RetainHistorical => {
                if query.at < state.changed_at {
                    current.push(record);
                }
            }
            MemoryRevalidationOutcome::Supersede => {}
        }
    }
    let requested_limit = query.limit;
    let candidate_limit = requested_limit.saturating_mul(4).min(100);
    let mut candidate_query = query.clone();
    candidate_query.limit = candidate_limit;
    let mut candidates = retrieval::recall(current.clone(), &candidate_query)?;
    append_semantic_candidates(context, &current, &query, candidate_limit, &mut candidates)?;
    if candidates.len() <= 1 {
        candidates.truncate(requested_limit as usize);
        return Ok(candidates);
    }

    let request = MemoryRankRequest {
        query: query.query,
        candidates: candidates
            .iter()
            .map(|record| MemoryRankCandidate {
                id: record.id.clone(),
                content: record.content.clone(),
            })
            .collect(),
        limit: requested_limit,
    };
    let Ok(response): Result<MemoryRankResponse, _> = context.sdk.rank.invoke_projected(&request)
    else {
        candidates.truncate(requested_limit as usize);
        return Ok(candidates);
    };

    let mut seen = std::collections::BTreeSet::new();
    let mut ranked = Vec::with_capacity(requested_limit as usize);
    for id in response.ids {
        if !seen.insert(id.clone()) {
            candidates.truncate(requested_limit as usize);
            return Ok(candidates);
        }
        let Some(record) = candidates.iter().find(|record| record.id == id) else {
            candidates.truncate(requested_limit as usize);
            return Ok(candidates);
        };
        ranked.push(record.clone());
        if ranked.len() == requested_limit as usize {
            return Ok(ranked);
        }
    }
    for record in candidates {
        if seen.insert(record.id.clone()) {
            ranked.push(record);
            if ranked.len() == requested_limit as usize {
                break;
            }
        }
    }
    Ok(ranked)
}

fn append_semantic_candidates(
    context: &MemoryContext<'_, '_>,
    current: &[MemoryRecord],
    query: &MemoryRecallQuery,
    limit: u32,
    candidates: &mut Vec<MemoryRecord>,
) -> MemoryResult<()> {
    if query.query.trim().is_empty() || limit == 0 {
        return Ok(());
    }

    let mut pool_query = query.clone();
    pool_query.query.clear();
    pool_query.limit = 100;
    let pool = retrieval::recall(current.to_vec(), &pool_query)?;
    if pool.is_empty() {
        return Ok(());
    }

    let inputs = std::iter::once(query.query.clone())
        .chain(pool.iter().map(|record| record.content.clone()))
        .collect();
    let Ok(response): Result<MemoryEmbeddingResponse, _> = context
        .sdk
        .embed
        .invoke_projected(&MemoryEmbeddingRequest { inputs })
    else {
        return Ok(());
    };
    if response.embeddings.len() != pool.len() + 1 {
        return Ok(());
    }
    let mut embeddings = response.embeddings.into_iter();
    let Some(query_embedding) = embeddings.next() else {
        return Ok(());
    };
    if query_embedding.is_empty() {
        return Ok(());
    }

    let mut scored = pool
        .into_iter()
        .zip(embeddings)
        .filter_map(|(record, embedding)| {
            cosine_similarity(&query_embedding, &embedding).map(|score| (score, record))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut seen = candidates
        .iter()
        .map(|record| record.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for (_, record) in scored.into_iter().take(limit as usize) {
        if seen.insert(record.id.clone()) {
            candidates.push(record);
        }
    }
    Ok(())
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let dot = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let left_norm = left.iter().map(|value| value * value).sum::<f64>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return None;
    }
    Some(dot / (left_norm * right_norm))
}

fn handle_compaction(
    context: &MemoryContext<'_, '_>,
    command: ContextCompactionCommand,
) -> MemoryResult<ContextCompactionResponse> {
    match command {
        ContextCompactionCommand::Compact { request } => {
            let checkpoint = compact_context(context, request)?;
            observe(
                context,
                COMPACTION_EVENT,
                serde_json::json!({ "checkpoint_id": checkpoint.id }),
            );
            Ok(ContextCompactionResponse::Compacted { checkpoint })
        }
    }
}

fn handle_expansion(
    context: &MemoryContext<'_, '_>,
    command: phenix_sdk::ContextExpansionCommand,
) -> MemoryResult<phenix_sdk::ContextExpansionResponse> {
    match command {
        phenix_sdk::ContextExpansionCommand::Expand {
            scope,
            checkpoint_id,
            depth,
        } => Ok(phenix_sdk::ContextExpansionResponse::Expanded {
            items: expand_checkpoint(context, &scope, &MemoryKey::parse(checkpoint_id)?, depth)?,
        }),
    }
}

fn expand_checkpoint(
    context: &MemoryContext<'_, '_>,
    scope: &MemoryScope,
    checkpoint_id: &MemoryKey,
    depth: u32,
) -> MemoryResult<Vec<phenix_sdk::CompactContextItem>> {
    let checkpoint_key = format!("checkpoint/{}", checkpoint_id.as_str());
    let checkpoint: ContextCheckpoint =
        read_record(context, &checkpoint_key)?.ok_or_else(|| {
            MemoryError::Missing(format!("context checkpoint {}", checkpoint_id.as_str()))
        })?;
    if &checkpoint.scope != scope {
        return Err(MemoryError::Invalid(
            "context checkpoint scope does not match expansion scope".into(),
        ));
    }

    let root_key = MemoryKey::parse(checkpoint.summary_node_id)?;
    let root: MemoryNode = read_record(context, &node_key(&root_key))?.ok_or_else(|| {
        MemoryError::Persistence(format!(
            "missing checkpoint summary node {}",
            root_key.as_str()
        ))
    })?;
    let mut level = vec![root];
    for _ in 0..depth {
        let mut children = Vec::new();
        for node in &level {
            for child_id in &node.children {
                let child_key = MemoryKey::parse(child_id.clone())?;
                let child = read_record(context, &node_key(&child_key))?.ok_or_else(|| {
                    MemoryError::Persistence(format!("missing child memory node {child_id}"))
                })?;
                children.push(child);
            }
        }
        if children.is_empty() {
            break;
        }
        level = children;
    }

    Ok(level
        .into_iter()
        .map(|node| phenix_sdk::CompactContextItem {
            id: node.id,
            content: node.summary,
            source_refs: node.source_refs,
            exact: false,
        })
        .collect())
}

fn compact_context(
    context: &MemoryContext<'_, '_>,
    request: ContextCompactionRequest,
) -> MemoryResult<ContextCheckpoint> {
    if request.target_tokens == 0 || request.items.is_empty() {
        return Err(MemoryError::Invalid(
            "context compaction requires items and a non-zero target token budget".into(),
        ));
    }

    let mut covered_refs = Vec::new();
    let mut retained_exact_refs = Vec::new();
    for item in &request.items {
        validate_text("context compaction item id", &item.id)?;
        validate_text("context compaction item content", &item.content)?;
        if item.source_refs.is_empty() {
            return Err(MemoryError::Invalid(format!(
                "context compaction item {} requires durable provenance",
                item.id
            )));
        }
        covered_refs.extend(item.source_refs.iter().cloned());
        if item.exact {
            retained_exact_refs.extend(item.source_refs.iter().cloned());
        }
    }
    normalize_sources(&mut covered_refs)?;
    normalize_sources(&mut retained_exact_refs)?;

    let encoded = serde_json::to_vec(&request)
        .map_err(|error| MemoryError::Persistence(error.to_string()))?;
    let digest = encoded
        .into_iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    let id = format!("checkpoint-{digest:016x}");
    let checkpoint_key = format!("checkpoint/{id}");
    if let Some(checkpoint) = read_record(context, &checkpoint_key)? {
        return Ok(checkpoint);
    }

    let input = serde_json::to_vec(&request.items)
        .map_err(|error| MemoryError::Provider(error.to_string()))?;
    let summary = routed_memory_text(
        context,
        &request.execution_id,
        &request.parent_attempt_id,
        &request.profile_id,
        memory_summarize_callable(),
        input,
        "memory summary",
    )?;

    let summary_node_id = format!("summary-{digest:016x}");
    record_node(
        context,
        MemoryNode {
            id: summary_node_id.clone(),
            scope: request.scope.clone(),
            summary: summary.clone(),
            children: Vec::new(),
            source_refs: covered_refs.clone(),
            created_at: 0,
            generation: 1,
        },
    )?;
    let checkpoint = ContextCheckpoint {
        scope: request.scope,
        id: id.clone(),
        summary,
        summary_node_id,
        covered_refs,
        retained_exact_refs,
        configuration_revision: request.configuration_revision,
    };
    insert_record(context, CHECKPOINT_INDEX, &checkpoint_key, &id, &checkpoint)?;
    Ok(checkpoint)
}

fn promote_memory(
    context: &MemoryContext<'_, '_>,
    id: String,
    promoted_id: String,
    scope: MemoryScope,
    created_at: u64,
) -> MemoryResult<MemoryRecord> {
    let source_id = MemoryKey::parse(id)?;
    let source: MemoryRecord = read_record(context, &record_key(&source_id))?
        .ok_or_else(|| MemoryError::Missing(format!("memory {}", source_id.as_str())))?;
    if !matches!(source.scope, MemoryScope::Session { .. }) {
        return Err(MemoryError::Invalid(
            "promotion requires a session-scoped source memory".into(),
        ));
    }
    if !matches!(
        scope,
        MemoryScope::Global | MemoryScope::Workspace { .. } | MemoryScope::Agent { .. }
    ) {
        return Err(MemoryError::Invalid(
            "promotion target must outlive the source session".into(),
        ));
    }

    record_memory(
        context,
        MemoryRecord {
            id: promoted_id,
            kind: source.kind,
            scope,
            content: source.content,
            source_refs: source.source_refs,
            supporting_dependencies: source.supporting_dependencies,
            supersedes: Vec::new(),
            valid_from: source.valid_from,
            valid_until: source.valid_until,
            created_at,
        },
    )
}

fn record_memory(
    context: &MemoryContext<'_, '_>,
    mut record: MemoryRecord,
) -> MemoryResult<MemoryRecord> {
    let id = MemoryKey::parse(record.id.clone())?;
    validate_scope(&record.scope)?;
    validate_text("memory content", &record.content)?;
    normalize_sources(&mut record.source_refs)?;
    normalize_supporting_dependencies(&mut record.supporting_dependencies)?;
    if record.source_refs.is_empty() {
        return Err(MemoryError::Invalid(
            "durable memory requires at least one source reference".into(),
        ));
    }
    if matches!((record.valid_from, record.valid_until), (Some(start), Some(end)) if start >= end) {
        return Err(MemoryError::Invalid(
            "valid_from must be earlier than valid_until".into(),
        ));
    }

    record.supersedes.sort();
    record.supersedes.dedup();
    let supersession_at = record
        .valid_from
        .unwrap_or(record.created_at)
        .max(record.created_at);
    let mut superseded_states = Vec::new();
    for prior_id in &record.supersedes {
        let prior_key = MemoryKey::parse(prior_id.clone())?;
        if prior_key == id {
            return Err(MemoryError::Invalid(
                "memory cannot supersede itself".into(),
            ));
        }
        let prior: MemoryRecord = read_record(context, &record_key(&prior_key))?
            .ok_or_else(|| MemoryError::Missing(format!("superseded memory {prior_id}")))?;
        if prior.kind != record.kind || prior.scope != record.scope {
            return Err(MemoryError::Invalid(format!(
                "superseded memory {prior_id} must have the same kind and scope"
            )));
        }
        let state_key = freshness_key(&prior_key);
        let mut state: MemoryFreshnessRecord =
            read_record(context, &state_key)?.unwrap_or_else(|| initial_state(&prior, None));
        if state.freshness == MemoryFreshness::Current {
            state.changed_at = supersession_at;
        } else {
            state.changed_at = state.changed_at.min(supersession_at);
        }
        state.freshness = MemoryFreshness::Historical;
        superseded_states.push((state_key, state));
    }

    let freshness = initial_state(&record, None);
    let freshness_key = freshness_key(&id);
    let dependencies = freshness
        .dependencies
        .iter()
        .map(|dependency| dependency_key(&dependency.service, &dependency.resource))
        .collect::<Vec<_>>();
    let updates = superseded_states
        .iter()
        .map(|(key, state)| (key.as_str(), state))
        .collect::<Vec<_>>();
    insert_record_with_sidecar_secondary_and_updates(
        context,
        RECORD_INDEX,
        &record_key(&id),
        id.as_str(),
        &record,
        Some((&freshness_key, &freshness)),
        Some((DEPENDENCY_INDEX, &dependencies)),
        &updates,
    )?;

    Ok(record)
}

fn record_node(context: &MemoryContext<'_, '_>, mut node: MemoryNode) -> MemoryResult<MemoryNode> {
    let id = MemoryKey::parse(node.id.clone())?;
    validate_scope(&node.scope)?;
    validate_text("memory node summary", &node.summary)?;
    normalize_sources(&mut node.source_refs)?;
    node.children.sort();
    node.children.dedup();
    if node.children.is_empty() && node.source_refs.is_empty() {
        return Err(MemoryError::Invalid(
            "memory node requires children or exact source references".into(),
        ));
    }

    for child_id in &node.children {
        let child_key = MemoryKey::parse(child_id.clone())?;
        if child_key == id {
            return Err(MemoryError::Invalid(
                "memory node cannot contain itself".into(),
            ));
        }
        let child: MemoryNode = read_record(context, &node_key(&child_key))?
            .ok_or_else(|| MemoryError::Missing(format!("child memory node {child_id}")))?;
        if child.scope != node.scope {
            return Err(MemoryError::Invalid(format!(
                "child memory node {child_id} must have the same scope"
            )));
        }
    }

    insert_record(context, NODE_INDEX, &node_key(&id), id.as_str(), &node)?;
    Ok(node)
}

fn expand_node(
    context: &MemoryContext<'_, '_>,
    id: &MemoryKey,
) -> MemoryResult<Option<MemoryExpansion>> {
    let Some(node) = read_record::<MemoryNode>(context, &node_key(id))? else {
        return Ok(None);
    };
    let children = node
        .children
        .iter()
        .map(|child_id| {
            let child_key = MemoryKey::parse(child_id.clone())?;
            read_record(context, &node_key(&child_key))?.ok_or_else(|| {
                MemoryError::Persistence(format!("missing child memory node {child_id}"))
            })
        })
        .collect::<MemoryResult<Vec<_>>>()?;
    Ok(Some(MemoryExpansion { node, children }))
}

fn validate_scope(scope: &MemoryScope) -> MemoryResult<()> {
    match scope {
        MemoryScope::Global | MemoryScope::Session { .. } => Ok(()),
        MemoryScope::Workspace { workspace_id } => validate_text("workspace id", workspace_id),
        MemoryScope::Agent { agent_id } => validate_text("agent id", agent_id),
    }
}

fn normalize_sources(sources: &mut Vec<MemorySourceReference>) -> MemoryResult<()> {
    for source in sources.iter() {
        validate_text("memory source resource", &source.resource)?;
        if source.service == memory_service()
            || source.service == context_compaction_service()
            || source.service == context_expansion_service()
        {
            return Err(MemoryError::Invalid(format!(
                "memory source {} must reference raw durable evidence, not derived memory",
                source.resource
            )));
        }
        if matches!((source.start, source.end), (Some(start), Some(end)) if start > end) {
            return Err(MemoryError::Invalid(format!(
                "memory source {} starts after it ends",
                source.resource
            )));
        }
    }
    sources.sort();
    sources.dedup();
    Ok(())
}

fn normalize_supporting_dependencies(
    dependencies: &mut Vec<MemoryDependencyRevision>,
) -> MemoryResult<()> {
    for dependency in dependencies.iter() {
        validate_text(
            "memory supporting dependency resource",
            &dependency.resource,
        )?;
        let revision = dependency.revision.as_deref().ok_or_else(|| {
            MemoryError::Invalid("memory supporting dependencies require an exact revision".into())
        })?;
        validate_text("memory supporting dependency revision", revision)?;
        if dependency.service == memory_service()
            || dependency.service == context_compaction_service()
            || dependency.service == context_expansion_service()
        {
            return Err(MemoryError::Invalid(format!(
                "memory supporting dependency {} must reference authoritative evidence, not derived memory",
                dependency.resource
            )));
        }
    }
    dependencies.sort();
    dependencies.dedup();
    Ok(())
}

fn validate_text(label: &str, value: &str) -> MemoryResult<()> {
    if value.trim().is_empty() {
        Err(MemoryError::Invalid(format!("{label} must not be empty")))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MemoryKey(String);

impl MemoryKey {
    fn parse(value: String) -> MemoryResult<Self> {
        if value.trim().is_empty() || value.contains('/') {
            return Err(MemoryError::Invalid(
                "memory id must be non-empty and must not contain '/'".into(),
            ));
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

fn record_key(id: &MemoryKey) -> String {
    format!("record/{}", id.as_str())
}

fn record_key_str(id: &str) -> String {
    format!("record/{id}")
}

fn freshness_key(id: &MemoryKey) -> String {
    format!("freshness/{}", id.as_str())
}

fn dependency_key(service: &ServiceId, resource: &str) -> String {
    serde_json::to_string(&(service.as_str(), resource))
        .expect("service id and resource strings always serialize")
}

fn node_key(id: &MemoryKey) -> String {
    format!("node/{}", id.as_str())
}

#[cfg(test)]
mod supporting_dependency_tests {
    use super::*;

    fn dependency(revision: Option<&str>) -> MemoryDependencyRevision {
        MemoryDependencyRevision {
            service: ServiceId::parse("phenix.language@1").unwrap(),
            resource: "entity/entity-1/body".into(),
            revision: revision.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn supporting_dependencies_require_exact_revisions() {
        let mut dependencies = vec![dependency(None)];
        assert!(matches!(
            normalize_supporting_dependencies(&mut dependencies),
            Err(MemoryError::Invalid(message))
                if message.contains("require an exact revision")
        ));
    }

    #[test]
    fn supporting_dependencies_are_deduplicated_without_losing_revision() {
        let exact = dependency(Some("body-revision-1"));
        let mut dependencies = vec![exact.clone(), exact.clone()];
        normalize_supporting_dependencies(&mut dependencies).unwrap();
        assert_eq!(dependencies, vec![exact]);
    }
}
