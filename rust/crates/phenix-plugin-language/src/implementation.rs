use phenix_core::{
    Authority, CapabilityId, ComponentInterface, DurableSchema, PluginContext, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ResourceNamespace, ServiceContribution,
    ServiceId, TransactionOp, ValueCodec,
};
use phenix_sdk::{
    CodeChangedNeighborhood, CodeEntityChangeEvent, CodeEntityChangePage, CodeEntityEditEvidence,
    CodeEntityEditResult, CodeEntityEditValidation, CodeEntityFacet, CodeEntityFacetChanges,
    CodeEntityInsertPosition, CodeEntityLineage, CodeEntityLineageConfidence,
    CodeEntityLineageKind, CodeEntityProviderEditValidationFactBatch, CodeEntityProviderFactBatch,
    CodeEntityProviderRelationFactBatch, CodeEntityRelationKind, CodeEntityRelations,
    CodeEntityRevision, CodeEntitySourceLocator, CodeEntitySourceView, CodeIdentityContinuityState,
    CodeIdentityContinuityStatus, CodeIdentityRebuildCheckpoint, CodePositionEncoding,
    CodeSourcePosition, CodeSourceRange, DiagnosticsResult, DocumentProvenance,
    FileRevisionFallback, LanguageCommand, LanguageDocumentIdentity, LanguageObservation,
    LanguageProviderEpoch, LanguageResponse, ProviderEpoch, WorkspaceCommand, WorkspaceFileVersion,
    WorkspaceInterface, WorkspaceResponse, WorkspaceWrite, LANGUAGE_SERVICE, WORKSPACE_SERVICE,
};
use phenix_sdk::{CodeEntityFacetRevisions, LanguageOperationKind, LogicalCodeEntity};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const LANGUAGE_PLUGIN: &str = "phenix.language";
const LANGUAGE_NAMESPACE: &str = "phenix.language.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const WORKSPACE_READ: &str = "workspace.read";
const WORKSPACE_WRITE: &str = "workspace.write";

#[derive(Default)]
struct LanguageState {
    providers: BTreeMap<String, LanguageProviderEpoch>,
    diagnostics: BTreeMap<String, DiagnosticsResult>,
}

type LanguageContext<'host, 'runtime, 'state> =
    PluginContext<'host, 'runtime, (), (), &'state mut LanguageState>;

fn context<'host, 'runtime, 'state>(
    host: &'host PluginHost<'runtime>,
    state: &'state mut LanguageState,
) -> LanguageContext<'host, 'runtime, 'state> {
    PluginContext::new(host, (), (), state)
}

#[must_use]
pub fn language_manifest() -> PluginManifest {
    PluginManifest {
        id: PluginId::parse(LANGUAGE_PLUGIN).expect("static plugin id is valid"),
        version: 1,
        execution: PluginExecution::Embedded,
        dependencies: Vec::new(),
        services: vec![ServiceContribution {
            role: phenix_core::ServiceRole::Terminal,
            service: language_service(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        resource_namespaces: vec![language_namespace()],
        maximum_authority: Authority::new([
            capability(PERSISTENCE_SCHEMA),
            capability(PERSISTENCE_READ),
            capability(PERSISTENCE_WRITE),
            capability(WORKSPACE_READ),
            capability(WORKSPACE_WRITE),
        ]),
    }
}

#[must_use]
pub fn language_factory() -> Box<dyn PluginInstance> {
    Box::new(LanguagePlugin::default())
}

#[must_use]
pub fn language_service() -> ServiceId {
    ServiceId::parse(LANGUAGE_SERVICE).expect("static service id is valid")
}

fn language_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(LANGUAGE_NAMESPACE).expect("static namespace is valid")
}

fn workspace_service() -> ServiceId {
    ServiceId::parse(WORKSPACE_SERVICE).expect("static workspace service id is valid")
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

#[derive(Default)]
struct LanguagePlugin {
    state: LanguageState,
}

impl PluginInstance for LanguagePlugin {
    fn start(&mut self, host: &PluginHost<'_>) -> Result<(), String> {
        context(host, &mut self.state)
            .kernel
            .register_durable_schema(&DurableSchema::new(language_namespace(), 1))
            .map_err(|error| error.to_string())
    }

    fn invoke(
        &mut self,
        service: &ServiceId,
        input: &[u8],
        host: &PluginHost<'_>,
    ) -> Result<Vec<u8>, String> {
        if service != &language_service() {
            return Err(format!("unsupported language service: {service}"));
        }
        let mut context = context(host, &mut self.state);
        let interface = crate::LanguageInterface::interface_id();
        let command = context
            .kernel
            .decode_projected::<LanguageCommand>(&interface, input)
            .map_err(|error| error.to_string())?;
        let response = handle(&mut context, command)?;
        context
            .kernel
            .encode_value(&response)
            .map_err(|error| error.to_string())
    }
}

fn handle(
    context: &mut LanguageContext<'_, '_, '_>,
    command: LanguageCommand,
) -> Result<LanguageResponse, String> {
    match command {
        LanguageCommand::ActivateProvider {
            workspace_id,
            provider_id,
            epoch,
        } => Ok(LanguageResponse::Provider {
            epoch: Some(activate_provider(
                context,
                workspace_id,
                provider_id,
                epoch,
            )?),
        }),
        LanguageCommand::EndProvider {
            workspace_id,
            provider_id,
            epoch,
        } => {
            require_epoch(context, &workspace_id, &provider_id, epoch)?;
            context.plugin.state.providers.remove(&workspace_id);
            context.plugin.state.diagnostics.remove(&workspace_id);
            Ok(LanguageResponse::Provider { epoch: None })
        }
        LanguageCommand::PublishDiagnostics {
            workspace_id,
            provider_id,
            epoch,
            result,
        } => {
            require_epoch(context, &workspace_id, &provider_id, epoch)?;
            validate_documents(result.documents())?;
            context
                .plugin
                .state
                .diagnostics
                .insert(workspace_id, result.clone());
            Ok(LanguageResponse::Diagnostics {
                result: Some(result),
            })
        }
        LanguageCommand::CurrentDiagnostics { workspace_id } => {
            validate_identity("workspace id", &workspace_id)?;
            Ok(LanguageResponse::Diagnostics {
                result: context.plugin.state.diagnostics.get(&workspace_id).cloned(),
            })
        }
        LanguageCommand::Consume {
            observation_id,
            execution_id,
            workspace_id,
            provider_id,
            epoch,
            result,
        } => {
            require_epoch(context, &workspace_id, &provider_id, epoch)?;
            validate_identity("language observation id", &observation_id)?;
            validate_identity("consuming execution id", &execution_id)?;
            validate_documents(&result.documents)?;
            let observation = LanguageObservation {
                id: observation_id,
                execution_id,
                workspace_id,
                provider_id,
                provider_epoch: epoch,
                result,
            };
            store_observation(context, &observation)?;
            Ok(LanguageResponse::Observation {
                observation: Some(observation),
            })
        }
        LanguageCommand::ReadFileFallback { workspace_id, path } => {
            validate_identity("workspace id", &workspace_id)?;
            validate_identity("language document path", &path)?;
            let input = context
                .kernel
                .encode_value(&WorkspaceCommand::Read { path: path.clone() })
                .map_err(|error| error.to_string())?;
            let output = context
                .kernel
                .invoke_service_abi(&workspace_service(), &input, context.call.authority, None)
                .map_err(|error| error.to_string())?;
            let response = context
                .kernel
                .decode_projected::<WorkspaceResponse>(&WorkspaceInterface::interface_id(), &output)
                .map_err(|error| error.to_string())?;
            let WorkspaceResponse::Read {
                path: observed_path,
                content,
                version,
            } = response
            else {
                return Err("workspace returned a non-read response to file fallback".into());
            };
            if observed_path != path {
                return Err(format!(
                    "workspace file fallback path mismatch: requested {path}, observed {observed_path}"
                ));
            }
            let WorkspaceFileVersion::Present { content_hash } = version else {
                return Err(format!(
                    "workspace file fallback is unavailable for absent path {path}"
                ));
            };
            Ok(LanguageResponse::FileFallback {
                fallback: FileRevisionFallback {
                    workspace_id,
                    document: LanguageDocumentIdentity {
                        path,
                        file_version: Some(workspace_revision_label(&content_hash)),
                        provenance: DocumentProvenance::WorkspaceBacked,
                    },
                    content,
                },
            })
        }
        LanguageCommand::RecordEntityRevision { revision } => {
            validate_code_entity_revision(&revision)?;
            store_entity_revision(context, &revision)?;
            Ok(LanguageResponse::EntityRevision {
                revision: Some(revision),
            })
        }
        LanguageCommand::IngestEntityFact {
            observation_id,
            fact_id,
        } => {
            validate_identity("language observation id", &observation_id)?;
            validate_identity("provider fact id", &fact_id)?;
            let revision = ingest_entity_fact(context, &observation_id, &fact_id)?;
            Ok(LanguageResponse::EntityRevision {
                revision: Some(revision),
            })
        }
        LanguageCommand::IngestDocumentSymbols {
            observation_id,
            repository_id,
        } => {
            validate_identity("language observation id", &observation_id)?;
            validate_identity("code repository id", &repository_id)?;
            Ok(LanguageResponse::EntityRevisions {
                revisions: ingest_document_symbol_observation(
                    context,
                    &observation_id,
                    &repository_id,
                    None,
                )?,
            })
        }
        LanguageCommand::IngestDocumentSymbolsWithEncoding {
            observation_id,
            repository_id,
            position_encoding,
        } => {
            validate_identity("language observation id", &observation_id)?;
            validate_identity("code repository id", &repository_id)?;
            Ok(LanguageResponse::EntityRevisions {
                revisions: ingest_document_symbol_observation(
                    context,
                    &observation_id,
                    &repository_id,
                    Some(position_encoding),
                )?,
            })
        }
        LanguageCommand::RecordEntityLineage {
            repository_id,
            lineage,
        } => {
            validate_code_entity_lineage(&repository_id, &lineage)?;
            store_entity_lineage(context, &repository_id, &lineage)?;
            Ok(LanguageResponse::EntityLineage {
                lineage: Some(lineage),
            })
        }
        LanguageCommand::GetEntityLineage {
            repository_id,
            from_entity_id,
            to_entity_id,
            kind,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("lineage source entity id", &from_entity_id)?;
            validate_identity("lineage target entity id", &to_entity_id)?;
            Ok(LanguageResponse::EntityLineage {
                lineage: read_entity_lineage(
                    context,
                    &repository_id,
                    &from_entity_id,
                    &to_entity_id,
                    kind,
                )?,
            })
        }
        LanguageCommand::GetEntityRevision {
            repository_id,
            entity_id,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            Ok(LanguageResponse::EntityRevision {
                revision: read_entity_revision(context, &repository_id, &entity_id)?,
            })
        }
        LanguageCommand::GetEntitySourceLocator {
            repository_id,
            entity_id,
            revision,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntitySourceLocator {
                locator: read_entity_source_locator(
                    context,
                    &repository_id,
                    &entity_id,
                    &revision,
                )?,
            })
        }
        LanguageCommand::ReadEntitySource {
            repository_id,
            entity_id,
            revision,
            max_bytes,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntitySource {
                view: read_entity_source(
                    context,
                    &repository_id,
                    &entity_id,
                    &revision,
                    max_bytes,
                )?,
            })
        }
        LanguageCommand::ReadEntityBody {
            repository_id,
            entity_id,
            revision,
            max_bytes,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntityBody {
                view: read_entity_body(context, &repository_id, &entity_id, &revision, max_bytes)?,
            })
        }
        LanguageCommand::IngestEntityRelations {
            observation_id,
            fact_id,
        } => {
            validate_identity("language observation id", &observation_id)?;
            validate_identity("provider relation fact id", &fact_id)?;
            Ok(LanguageResponse::EntityRelations {
                relations: Some(ingest_entity_relations(context, &observation_id, &fact_id)?),
            })
        }
        LanguageCommand::ReadEntityRelations {
            repository_id,
            entity_id,
            revision,
            kind,
            max_items,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntityRelations {
                relations: read_entity_relations(
                    context,
                    &repository_id,
                    &entity_id,
                    &revision,
                    kind,
                    max_items,
                )?,
            })
        }
        LanguageCommand::ReadChangedNeighborhood {
            repository_id,
            entity_id,
            from_revision,
            max_items,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &from_revision)?;
            Ok(LanguageResponse::ChangedNeighborhood {
                neighborhood: read_changed_neighborhood(
                    context,
                    &repository_id,
                    &entity_id,
                    &from_revision,
                    max_items,
                )?,
            })
        }
        LanguageCommand::IngestEditValidation {
            observation_id,
            fact_id,
        } => {
            validate_identity("language observation id", &observation_id)?;
            validate_identity("provider edit validation fact id", &fact_id)?;
            Ok(LanguageResponse::EditValidation {
                validation: Some(ingest_edit_validation(context, &observation_id, &fact_id)?),
            })
        }
        LanguageCommand::ReplaceEntityBody {
            operation_id,
            repository_id,
            entity_id,
            revision,
            content,
        } => {
            validate_identity("semantic edit operation id", &operation_id)?;
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntityEdit {
                result: replace_entity_body(
                    context,
                    operation_id,
                    &repository_id,
                    &entity_id,
                    &revision,
                    content,
                )?,
            })
        }
        LanguageCommand::InsertRelativeToEntity {
            operation_id,
            repository_id,
            entity_id,
            revision,
            position,
            content,
        } => {
            validate_identity("semantic edit operation id", &operation_id)?;
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntityEdit {
                result: insert_relative_to_entity(
                    context,
                    operation_id,
                    &repository_id,
                    &entity_id,
                    &revision,
                    position,
                    content,
                )?,
            })
        }
        LanguageCommand::RemoveEntity {
            operation_id,
            repository_id,
            entity_id,
            revision,
        } => {
            validate_identity("semantic edit operation id", &operation_id)?;
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &revision)?;
            Ok(LanguageResponse::EntityEdit {
                result: remove_entity(
                    context,
                    operation_id,
                    &repository_id,
                    &entity_id,
                    &revision,
                )?,
            })
        }
        LanguageCommand::GetEntityFacet {
            repository_id,
            entity_id,
            facet,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            if let CodeEntityFacet::Relation { name } = &facet {
                validate_identity("relation facet", name)?;
            }
            let reference = read_entity_revision(context, &repository_id, &entity_id)?
                .and_then(|revision| revision.facet_reference(facet));
            Ok(LanguageResponse::EntityFacet { reference })
        }
        LanguageCommand::GetEntityFacetChanges {
            repository_id,
            entity_id,
            from_revision,
        } => {
            validate_identity("code repository id", &repository_id)?;
            validate_identity("logical code entity id", &entity_id)?;
            validate_identity("code entity revision", &from_revision)?;
            let current = read_entity_revision(context, &repository_id, &entity_id)?;
            let previous =
                read_entity_revision_version(context, &repository_id, &entity_id, &from_revision)?;
            let changes = match (previous, current) {
                (Some(previous), Some(current)) => Some(CodeEntityFacetChanges::between(
                    &previous.facets,
                    &current.facets,
                )),
                _ => None,
            };
            Ok(LanguageResponse::EntityFacetChanges { changes })
        }
        LanguageCommand::GetEntityChanges {
            repository_id,
            after_sequence,
            limit,
        } => {
            validate_identity("code repository id", &repository_id)?;
            Ok(LanguageResponse::EntityChanges {
                page: read_entity_changes(context, &repository_id, after_sequence, limit)?,
            })
        }
        LanguageCommand::SetIdentityContinuity { state } => {
            validate_identity("code repository id", &state.repository_id)?;
            if let Some(reason) = &state.reason {
                validate_identity("identity continuity reason", reason)?;
            }
            if state.status == CodeIdentityContinuityStatus::Rebuilding {
                return Err(
                    "rebuilding continuity must be entered through BeginIdentityRebuild".into(),
                );
            }
            store_identity_continuity(context, &state)?;
            Ok(LanguageResponse::IdentityContinuity { state: Some(state) })
        }
        LanguageCommand::BeginIdentityRebuild { repository_id } => {
            validate_identity("code repository id", &repository_id)?;
            Ok(LanguageResponse::IdentityRebuild {
                checkpoint: begin_identity_rebuild(context, &repository_id)?,
            })
        }
        LanguageCommand::CompleteIdentityRebuild {
            repository_id,
            applied_through_sequence,
        } => {
            validate_identity("code repository id", &repository_id)?;
            Ok(LanguageResponse::IdentityRebuild {
                checkpoint: complete_identity_rebuild(
                    context,
                    &repository_id,
                    applied_through_sequence,
                )?,
            })
        }
        LanguageCommand::GetIdentityContinuity { repository_id } => {
            validate_identity("code repository id", &repository_id)?;
            Ok(LanguageResponse::IdentityContinuity {
                state: read_identity_continuity(context, &repository_id)?,
            })
        }
        LanguageCommand::GetObservation { observation_id } => {
            validate_identity("language observation id", &observation_id)?;
            Ok(LanguageResponse::Observation {
                observation: read_observation(context, &observation_id)?,
            })
        }
    }
}

fn activate_provider(
    context: &mut LanguageContext<'_, '_, '_>,
    workspace_id: String,
    provider_id: String,
    epoch: ProviderEpoch,
) -> Result<LanguageProviderEpoch, String> {
    validate_identity("workspace id", &workspace_id)?;
    validate_identity("language provider id", &provider_id)?;
    if let Some(current) = context.plugin.state.providers.get(&workspace_id) {
        if epoch <= current.epoch {
            return Err(format!(
                "language provider epoch must advance beyond {}",
                current.epoch.get()
            ));
        }
    }
    let active = LanguageProviderEpoch {
        workspace_id: workspace_id.clone(),
        provider_id,
        epoch,
    };
    context
        .plugin
        .state
        .providers
        .insert(workspace_id.clone(), active.clone());
    context.plugin.state.diagnostics.remove(&workspace_id);
    Ok(active)
}

fn require_epoch(
    context: &LanguageContext<'_, '_, '_>,
    workspace_id: &str,
    provider_id: &str,
    epoch: ProviderEpoch,
) -> Result<(), String> {
    validate_identity("workspace id", workspace_id)?;
    validate_identity("language provider id", provider_id)?;
    match context.plugin.state.providers.get(workspace_id) {
        Some(active) if active.provider_id == provider_id && active.epoch == epoch => Ok(()),
        Some(active) => Err(format!(
            "ProviderChanged: active provider is {} epoch {}",
            active.provider_id,
            active.epoch.get()
        )),
        None => Err("ProviderChanged: no active provider".into()),
    }
}

fn store_observation(
    context: &LanguageContext<'_, '_, '_>,
    observation: &LanguageObservation,
) -> Result<(), String> {
    let key = observation_key(&observation.id);
    let encoded = serde_json::to_vec(observation).map_err(|error| error.to_string())?;
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::Put {
                    key,
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_observation(
    context: &LanguageContext<'_, '_, '_>,
    observation_id: &str,
) -> Result<Option<LanguageObservation>, String> {
    context
        .kernel
        .read_durable(&language_namespace(), &observation_key(observation_id))
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn ingest_entity_fact(
    context: &LanguageContext<'_, '_, '_>,
    observation_id: &str,
    fact_id: &str,
) -> Result<CodeEntityRevision, String> {
    let observation = read_observation(context, observation_id)?
        .ok_or_else(|| format!("unknown language observation: {observation_id}"))?;
    if !matches!(
        observation.result.operation,
        phenix_sdk::LanguageOperationKind::Definition
            | phenix_sdk::LanguageOperationKind::References
            | phenix_sdk::LanguageOperationKind::Implementations
            | phenix_sdk::LanguageOperationKind::DocumentSymbols
            | phenix_sdk::LanguageOperationKind::WorkspaceSymbols
            | phenix_sdk::LanguageOperationKind::CallHierarchy
    ) {
        return Err("language observation does not contain reusable semantic code facts".into());
    }

    let payload = serde_json::Value::from_value(&observation.result.payload)
        .map_err(|error| format!("provider fact payload is not JSON-compatible: {error}"))?;
    let batch: CodeEntityProviderFactBatch = serde_json::from_value(payload)
        .map_err(|error| format!("provider fact payload is invalid: {error}"))?;
    let fact = batch
        .facts
        .into_iter()
        .find(|fact| fact.id == fact_id)
        .ok_or_else(|| format!("provider fact not found in observation: {fact_id}"))?;

    let document_index = usize::try_from(fact.document_index)
        .map_err(|_| "provider fact document index is out of range".to_owned())?;
    let document = observation
        .result
        .documents
        .get(document_index)
        .cloned()
        .ok_or_else(|| "provider fact document index is out of range".to_owned())?;
    if document.provenance != DocumentProvenance::WorkspaceBacked {
        return Err("provider fact requires workspace-backed source provenance".into());
    }
    let expected_version = document
        .file_version
        .as_deref()
        .ok_or_else(|| "provider fact requires an exact workspace source revision".to_owned())?;
    verify_workspace_document_revision(context, &document.path, expected_version)?;
    let source = fact.source.clone();

    let revision = CodeEntityRevision {
        entity: fact.entity,
        revision: fact.revision,
        sequence: fact.sequence,
        document,
        symbol: fact.symbol,
        name: fact.name,
        signature_identity: fact.signature_identity,
        body_identity: fact.body_identity,
        provider_id: observation.provider_id,
        provider_epoch: observation.provider_epoch,
        facets: fact.facets,
    };
    validate_code_entity_revision(&revision)?;
    store_entity_revision(context, &revision)?;
    if let Some(source) = source {
        validate_code_source_range(&source.range, "provider source range")?;
        validate_code_source_range(&source.selection_range, "provider selection range")?;
        if !code_range_contains(&source.range, &source.selection_range) {
            return Err("provider selection range must be inside its source range".into());
        }
        if let Some(body_range) = &source.body_range {
            validate_code_source_range(body_range, "provider body range")?;
            if !code_range_contains(&source.range, body_range) {
                return Err("provider body range must be inside its source range".into());
            }
        }
        store_entity_source_locator(
            context,
            &CodeEntitySourceLocator {
                entity: revision.entity.clone(),
                revision: revision.revision.clone(),
                document: revision.document.clone(),
                provider_id: revision.provider_id.clone(),
                provider_epoch: revision.provider_epoch,
                position_encoding: source.position_encoding,
                range: source.range,
                selection_range: source.selection_range,
                body_range: source.body_range,
            },
        )?;
    }
    Ok(revision)
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LspPosition {
    line: u32,
    character: u32,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LspDocumentSymbol {
    name: String,
    #[serde(default)]
    detail: Option<String>,
    kind: u32,
    range: LspRange,
    #[serde(rename = "selectionRange")]
    selection_range: LspRange,
    #[serde(default)]
    children: Option<Vec<LspDocumentSymbol>>,
}

fn ingest_document_symbol_observation(
    context: &LanguageContext<'_, '_, '_>,
    observation_id: &str,
    repository_id: &str,
    position_encoding: Option<CodePositionEncoding>,
) -> Result<Vec<CodeEntityRevision>, String> {
    let observation = read_observation(context, observation_id)?
        .ok_or_else(|| format!("unknown language observation: {observation_id}"))?;
    if observation.result.operation != LanguageOperationKind::DocumentSymbols {
        return Err("document-symbol ingestion requires a document_symbols observation".into());
    }
    if observation.result.documents.len() != 1 {
        return Err("document-symbol ingestion requires exactly one source document".into());
    }

    let document = observation.result.documents[0].clone();
    if document.provenance != DocumentProvenance::WorkspaceBacked {
        return Err("document-symbol ingestion requires workspace-backed source provenance".into());
    }
    let source_revision = document.file_version.as_deref().ok_or_else(|| {
        "document-symbol ingestion requires an exact workspace source revision".to_owned()
    })?;
    verify_workspace_document_revision(context, &document.path, source_revision)?;

    let symbols = parse_lsp_document_symbols(&observation.result.payload)?;
    let mut revisions = Vec::new();
    let mut seen = BTreeSet::new();
    let mut parents = Vec::new();
    for symbol in &symbols {
        ingest_lsp_document_symbol(
            context,
            &observation,
            repository_id,
            &document,
            source_revision,
            symbol,
            position_encoding,
            &mut parents,
            &mut seen,
            &mut revisions,
        )?;
    }
    Ok(revisions)
}

fn parse_lsp_document_symbols(
    payload: &phenix_core::PhenixValue,
) -> Result<Vec<LspDocumentSymbol>, String> {
    let value = serde_json::Value::from_value(payload)
        .map_err(|error| format!("document-symbol payload is not JSON-compatible: {error}"))?;
    let symbols = match value {
        serde_json::Value::Null => return Ok(Vec::new()),
        serde_json::Value::Array(_) => value,
        serde_json::Value::Object(mut object) => object
            .remove("symbols")
            .ok_or_else(|| "document-symbol payload must be an LSP symbol array".to_owned())?,
        _ => return Err("document-symbol payload must be an LSP symbol array".into()),
    };
    serde_json::from_value(symbols)
        .map_err(|error| format!("document-symbol payload is invalid: {error}"))
}

#[allow(clippy::too_many_arguments)]
fn ingest_lsp_document_symbol(
    context: &LanguageContext<'_, '_, '_>,
    observation: &LanguageObservation,
    repository_id: &str,
    document: &LanguageDocumentIdentity,
    source_revision: &str,
    symbol: &LspDocumentSymbol,
    position_encoding: Option<CodePositionEncoding>,
    parents: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    revisions: &mut Vec<CodeEntityRevision>,
) -> Result<(), String> {
    validate_identity("LSP document symbol name", &symbol.name)?;
    if symbol.kind == 0 {
        return Err("LSP document symbol kind must be non-zero".into());
    }
    validate_lsp_range(&symbol.range)?;
    validate_lsp_range(&symbol.selection_range)?;
    if !range_contains(&symbol.range, &symbol.selection_range) {
        return Err("LSP document symbol selection range must be inside its full range".into());
    }

    let mut path = parents.clone();
    path.push(symbol.name.clone());
    let semantic_path = path.join("::");
    let detail = symbol
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty())
        .unwrap_or("");
    let semantic_key = digest_identity(
        "lsp-symbol",
        &[
            repository_id.to_owned(),
            document.path.clone(),
            semantic_path.clone(),
            symbol.kind.to_string(),
            detail.to_owned(),
        ],
    );
    if !seen.insert(semantic_key.clone()) {
        return Err(format!(
            "ambiguous duplicate LSP document symbol identity: {semantic_path}"
        ));
    }

    let entity_id = digest_identity(
        "code-entity",
        &[repository_id.to_owned(), semantic_key.clone()],
    );
    let name_location = digest_identity(
        "code-name-location",
        &[
            document.path.clone(),
            semantic_path.clone(),
            lsp_range_identity(&symbol.range),
            lsp_range_identity(&symbol.selection_range),
        ],
    );
    let signature_identity = (!detail.is_empty()).then(|| {
        digest_identity(
            "code-signature",
            &[symbol.kind.to_string(), detail.to_owned()],
        )
    });
    let revision_id = digest_identity(
        "code-revision",
        &[
            entity_id.clone(),
            source_revision.to_owned(),
            name_location.clone(),
            signature_identity.clone().unwrap_or_default(),
        ],
    );
    let entity = LogicalCodeEntity {
        id: entity_id,
        repository_id: repository_id.to_owned(),
    };
    let current = read_entity_revision(context, repository_id, &entity.id)?;
    if let Some(current) = current.as_ref() {
        if current.revision == revision_id {
            if let Some(position_encoding) = position_encoding {
                store_entity_source_locator(
                    context,
                    &entity_source_locator(
                        current,
                        observation,
                        position_encoding,
                        &symbol.range,
                        &symbol.selection_range,
                    ),
                )?;
            }
            revisions.push(current.clone());
            ingest_lsp_children(
                context,
                observation,
                repository_id,
                document,
                source_revision,
                symbol,
                position_encoding,
                parents,
                seen,
                revisions,
            )?;
            return Ok(());
        }
    }
    let sequence = current.as_ref().map_or(Ok(1), |current| {
        current
            .sequence
            .checked_add(1)
            .ok_or_else(|| "code entity revision sequence overflow".to_owned())
    })?;
    let revision = CodeEntityRevision {
        entity: entity.clone(),
        revision: revision_id,
        sequence,
        document: document.clone(),
        symbol: Some(semantic_path),
        name: symbol.name.clone(),
        signature_identity: signature_identity.clone(),
        body_identity: None,
        provider_id: observation.provider_id.clone(),
        provider_epoch: observation.provider_epoch,
        facets: CodeEntityFacetRevisions {
            existence: digest_identity(
                "code-existence",
                &[
                    repository_id.to_owned(),
                    entity.id.clone(),
                    "present".into(),
                ],
            ),
            name_location,
            signature: signature_identity,
            body: None,
            relations: BTreeMap::new(),
        },
    };
    validate_code_entity_revision(&revision)?;
    store_entity_revision(context, &revision)?;
    if let Some(position_encoding) = position_encoding {
        store_entity_source_locator(
            context,
            &entity_source_locator(
                &revision,
                observation,
                position_encoding,
                &symbol.range,
                &symbol.selection_range,
            ),
        )?;
    }
    revisions.push(revision);

    ingest_lsp_children(
        context,
        observation,
        repository_id,
        document,
        source_revision,
        symbol,
        position_encoding,
        parents,
        seen,
        revisions,
    )
}

#[allow(clippy::too_many_arguments)]
fn ingest_lsp_children(
    context: &LanguageContext<'_, '_, '_>,
    observation: &LanguageObservation,
    repository_id: &str,
    document: &LanguageDocumentIdentity,
    source_revision: &str,
    symbol: &LspDocumentSymbol,
    position_encoding: Option<CodePositionEncoding>,
    parents: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
    revisions: &mut Vec<CodeEntityRevision>,
) -> Result<(), String> {
    let Some(children) = symbol.children.as_deref() else {
        return Ok(());
    };
    parents.push(symbol.name.clone());
    for child in children {
        ingest_lsp_document_symbol(
            context,
            observation,
            repository_id,
            document,
            source_revision,
            child,
            position_encoding,
            parents,
            seen,
            revisions,
        )?;
    }
    parents.pop();
    Ok(())
}

fn entity_source_locator(
    revision: &CodeEntityRevision,
    observation: &LanguageObservation,
    position_encoding: CodePositionEncoding,
    range: &LspRange,
    selection_range: &LspRange,
) -> CodeEntitySourceLocator {
    CodeEntitySourceLocator {
        entity: revision.entity.clone(),
        revision: revision.revision.clone(),
        document: revision.document.clone(),
        provider_id: observation.provider_id.clone(),
        provider_epoch: observation.provider_epoch,
        position_encoding,
        range: code_source_range(range),
        selection_range: code_source_range(selection_range),
        body_range: None,
    }
}

fn code_source_range(range: &LspRange) -> CodeSourceRange {
    CodeSourceRange {
        start: CodeSourcePosition {
            line: range.start.line,
            character: range.start.character,
        },
        end: CodeSourcePosition {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn store_entity_source_locator(
    context: &LanguageContext<'_, '_, '_>,
    locator: &CodeEntitySourceLocator,
) -> Result<(), String> {
    let key = entity_source_locator_key(
        &locator.entity.repository_id,
        &locator.entity.id,
        &locator.revision,
    );
    let encoded = serde_json::to_vec(locator).map_err(|error| error.to_string())?;
    if let Some(existing) = context
        .kernel
        .read_durable(&language_namespace(), &key)
        .map_err(|error| error.to_string())?
    {
        let existing: CodeEntitySourceLocator =
            serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
        if existing == *locator {
            return Ok(());
        }
        return Err(format!(
            "code entity source locator {} already exists with different content",
            locator.revision
        ));
    }
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::Put {
                    key,
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_entity_source_locator(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
) -> Result<Option<CodeEntitySourceLocator>, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &entity_source_locator_key(repository_id, entity_id, revision),
        )
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_entity_source(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    max_bytes: u64,
) -> Result<Option<CodeEntitySourceView>, String> {
    let Some(locator) = read_entity_source_locator(context, repository_id, entity_id, revision)?
    else {
        return Ok(None);
    };
    let range = locator.range.clone();
    read_entity_range(context, locator, range, max_bytes, "entity source")
}

fn read_entity_body(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    max_bytes: u64,
) -> Result<Option<CodeEntitySourceView>, String> {
    let Some(locator) = read_entity_source_locator(context, repository_id, entity_id, revision)?
    else {
        return Ok(None);
    };
    let range = locator
        .body_range
        .clone()
        .ok_or_else(|| "code entity has no exact semantic body range".to_owned())?;
    read_entity_range(context, locator, range, max_bytes, "entity body")
}

fn relation_operation(kind: CodeEntityRelationKind) -> LanguageOperationKind {
    match kind {
        CodeEntityRelationKind::Callers => LanguageOperationKind::CallHierarchy,
        CodeEntityRelationKind::References => LanguageOperationKind::References,
        CodeEntityRelationKind::Implementations => LanguageOperationKind::Implementations,
    }
}

fn relation_kind_key(kind: CodeEntityRelationKind) -> &'static str {
    match kind {
        CodeEntityRelationKind::Callers => "callers",
        CodeEntityRelationKind::References => "references",
        CodeEntityRelationKind::Implementations => "implementations",
    }
}

fn entity_relations_key(
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    kind: CodeEntityRelationKind,
) -> String {
    format!(
        "entity/{repository_id}/{entity_id}/relations/{revision}/{}",
        relation_kind_key(kind)
    )
}

fn ingest_entity_relations(
    context: &LanguageContext<'_, '_, '_>,
    observation_id: &str,
    fact_id: &str,
) -> Result<CodeEntityRelations, String> {
    let observation = read_observation(context, observation_id)?
        .ok_or_else(|| format!("unknown language observation: {observation_id}"))?;
    let payload = serde_json::Value::from_value(&observation.result.payload)
        .map_err(|error| format!("provider relation payload is not JSON-compatible: {error}"))?;
    let batch: CodeEntityProviderRelationFactBatch = serde_json::from_value(payload)
        .map_err(|error| format!("provider relation payload is invalid: {error}"))?;
    let fact = batch
        .facts
        .into_iter()
        .find(|fact| fact.id == fact_id)
        .ok_or_else(|| format!("provider relation fact not found in observation: {fact_id}"))?;

    if observation.result.operation != relation_operation(fact.kind) {
        return Err(format!(
            "provider relation fact {:?} does not match observation operation {:?}",
            fact.kind, observation.result.operation
        ));
    }
    validate_identity("code repository id", &fact.entity.repository_id)?;
    validate_identity("logical code entity id", &fact.entity.id)?;
    validate_identity("code entity revision", &fact.revision)?;
    let current = read_entity_revision(context, &fact.entity.repository_id, &fact.entity.id)?
        .ok_or_else(|| {
            format!(
                "unknown logical code entity: {}/{}",
                fact.entity.repository_id, fact.entity.id
            )
        })?;
    if current.revision != fact.revision {
        return Err(format!(
            "relation fact revision is stale: expected {}, current {}",
            fact.revision, current.revision
        ));
    }
    if current.provider_id != observation.provider_id
        || current.provider_epoch != observation.provider_epoch
    {
        return Err("relation fact provider no longer owns the current entity revision".into());
    }

    let mut targets = fact.targets;
    targets.sort_by(|left, right| {
        left.entity
            .repository_id
            .cmp(&right.entity.repository_id)
            .then_with(|| left.entity.id.cmp(&right.entity.id))
            .then_with(|| left.revision.cmp(&right.revision))
    });
    targets.dedup();
    for target in &targets {
        validate_identity(
            "relation target repository id",
            &target.entity.repository_id,
        )?;
        validate_identity("relation target entity id", &target.entity.id)?;
        if let Some(revision) = &target.revision {
            validate_identity("relation target revision", revision)?;
        }
    }

    let relations = CodeEntityRelations {
        entity: fact.entity,
        revision: fact.revision,
        kind: fact.kind,
        targets,
        complete: fact.complete,
    };
    let key = entity_relations_key(
        &relations.entity.repository_id,
        &relations.entity.id,
        &relations.revision,
        relations.kind,
    );
    let encoded = serde_json::to_vec(&relations).map_err(|error| error.to_string())?;
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: context
                        .kernel
                        .read_durable(&language_namespace(), &key)
                        .map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key,
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(relations)
}

fn read_entity_relations(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    kind: CodeEntityRelationKind,
    max_items: u32,
) -> Result<Option<CodeEntityRelations>, String> {
    if max_items == 0 {
        return Err("semantic relation read requires a non-zero item bound".into());
    }
    let key = entity_relations_key(repository_id, entity_id, revision, kind);
    let Some(encoded) = context
        .kernel
        .read_durable(&language_namespace(), &key)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let mut relations: CodeEntityRelations =
        serde_json::from_slice(&encoded).map_err(|error| error.to_string())?;
    let limit = usize::try_from(max_items).unwrap_or(usize::MAX);
    if relations.targets.len() > limit {
        relations.targets.truncate(limit);
        relations.complete = false;
    }
    Ok(Some(relations))
}

fn read_changed_neighborhood(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    from_revision: &str,
    max_items: u32,
) -> Result<Option<CodeChangedNeighborhood>, String> {
    if max_items == 0 {
        return Err("changed-neighborhood read requires a non-zero item bound".into());
    }
    let Some(previous) =
        read_entity_revision_version(context, repository_id, entity_id, from_revision)?
    else {
        return Ok(None);
    };
    let Some(current) = read_entity_revision(context, repository_id, entity_id)? else {
        return Ok(None);
    };
    let changes = CodeEntityFacetChanges::between(&previous.facets, &current.facets);
    let mut remaining = usize::try_from(max_items).unwrap_or(usize::MAX);
    let mut complete = true;
    let mut relations = Vec::new();
    for kind in [
        CodeEntityRelationKind::Callers,
        CodeEntityRelationKind::References,
        CodeEntityRelationKind::Implementations,
    ] {
        if remaining == 0 {
            complete = false;
            break;
        }
        let bound = u32::try_from(remaining).unwrap_or(u32::MAX);
        match read_entity_relations(
            context,
            repository_id,
            entity_id,
            &current.revision,
            kind,
            bound,
        )? {
            Some(value) => {
                remaining = remaining.saturating_sub(value.targets.len());
                complete &= value.complete;
                relations.push(value);
            }
            None => complete = false,
        }
    }
    Ok(Some(CodeChangedNeighborhood {
        entity: current.entity,
        from_revision: previous.revision,
        current_revision: current.revision,
        changes,
        relations,
        complete,
    }))
}

fn edit_validation_key(repository_id: &str, entity_id: &str, operation_id: &str) -> String {
    let operation = format!("{:x}", Sha256::digest(operation_id.as_bytes()));
    format!("entity/{repository_id}/{entity_id}/edit-validation/{operation}")
}

fn ingest_edit_validation(
    context: &LanguageContext<'_, '_, '_>,
    observation_id: &str,
    fact_id: &str,
) -> Result<CodeEntityEditValidation, String> {
    let observation = read_observation(context, observation_id)?
        .ok_or_else(|| format!("unknown language observation: {observation_id}"))?;
    if observation.result.operation != LanguageOperationKind::EditValidation {
        return Err("edit validation ingestion requires an edit_validation observation".into());
    }
    let payload = serde_json::Value::from_value(&observation.result.payload)
        .map_err(|error| format!("edit validation payload is not JSON-compatible: {error}"))?;
    let batch: CodeEntityProviderEditValidationFactBatch = serde_json::from_value(payload)
        .map_err(|error| format!("edit validation payload is invalid: {error}"))?;
    let fact = batch
        .facts
        .into_iter()
        .find(|fact| fact.id == fact_id)
        .ok_or_else(|| format!("edit validation fact not found in observation: {fact_id}"))?;
    if !fact.valid {
        return Err("provider rejected semantic edit structure validation".into());
    }
    validate_identity("semantic edit operation id", &fact.operation_id)?;
    validate_identity("code repository id", &fact.entity.repository_id)?;
    validate_identity("logical code entity id", &fact.entity.id)?;
    validate_identity("code entity revision", &fact.revision)?;
    validate_identity("semantic edit intent identity", &fact.intent_identity)?;

    let current = read_entity_revision(context, &fact.entity.repository_id, &fact.entity.id)?
        .ok_or_else(|| {
            format!(
                "unknown logical code entity: {}/{}",
                fact.entity.repository_id, fact.entity.id
            )
        })?;
    if current.revision != fact.revision {
        return Err("edit validation targets a stale entity revision".into());
    }
    if current.provider_id != observation.provider_id
        || current.provider_epoch != observation.provider_epoch
    {
        return Err("edit validation provider no longer owns the current entity revision".into());
    }
    let validation = CodeEntityEditValidation {
        operation_id: fact.operation_id,
        entity: fact.entity,
        revision: fact.revision,
        intent_identity: fact.intent_identity,
        provider_id: observation.provider_id,
        provider_epoch: observation.provider_epoch,
    };
    let key = edit_validation_key(
        &validation.entity.repository_id,
        &validation.entity.id,
        &validation.operation_id,
    );
    let encoded = serde_json::to_vec(&validation).map_err(|error| error.to_string())?;
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: context
                        .kernel
                        .read_durable(&language_namespace(), &key)
                        .map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key,
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(validation)
}

fn read_edit_validation(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    operation_id: &str,
) -> Result<Option<CodeEntityEditValidation>, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &edit_validation_key(repository_id, entity_id, operation_id),
        )
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

#[derive(Clone, Copy)]
enum SemanticEditTarget {
    Body,
    BeforeEntity,
    AfterEntity,
    Entity,
}

impl SemanticEditTarget {
    const fn label(self) -> &'static str {
        match self {
            Self::Body => "replace_body",
            Self::BeforeEntity => "insert_before",
            Self::AfterEntity => "insert_after",
            Self::Entity => "remove_entity",
        }
    }
}

fn semantic_edit_intent_identity(target: SemanticEditTarget, replacement: &str) -> String {
    digest_identity(
        "semantic-edit-intent",
        &[target.label().to_owned(), replacement.to_owned()],
    )
}

fn replace_entity_body(
    context: &LanguageContext<'_, '_, '_>,
    operation_id: String,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    replacement: String,
) -> Result<CodeEntityEditResult, String> {
    edit_entity_source(
        context,
        operation_id,
        repository_id,
        entity_id,
        revision,
        SemanticEditTarget::Body,
        replacement,
    )
}

fn insert_relative_to_entity(
    context: &LanguageContext<'_, '_, '_>,
    operation_id: String,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    position: CodeEntityInsertPosition,
    content: String,
) -> Result<CodeEntityEditResult, String> {
    let target = match position {
        CodeEntityInsertPosition::Before => SemanticEditTarget::BeforeEntity,
        CodeEntityInsertPosition::After => SemanticEditTarget::AfterEntity,
    };
    edit_entity_source(
        context,
        operation_id,
        repository_id,
        entity_id,
        revision,
        target,
        content,
    )
}

fn remove_entity(
    context: &LanguageContext<'_, '_, '_>,
    operation_id: String,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
) -> Result<CodeEntityEditResult, String> {
    edit_entity_source(
        context,
        operation_id,
        repository_id,
        entity_id,
        revision,
        SemanticEditTarget::Entity,
        String::new(),
    )
}

fn edit_entity_source(
    context: &LanguageContext<'_, '_, '_>,
    operation_id: String,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
    target: SemanticEditTarget,
    replacement: String,
) -> Result<CodeEntityEditResult, String> {
    let current = read_entity_revision(context, repository_id, entity_id)?
        .ok_or_else(|| format!("unknown logical code entity: {repository_id}/{entity_id}"))?;
    if current.revision != revision {
        return Err(format!(
            "code entity revision is stale: expected {revision}, current {}",
            current.revision
        ));
    }

    let locator = read_entity_source_locator(context, repository_id, entity_id, revision)?
        .ok_or_else(|| format!("code entity revision has no exact source locator: {revision}"))?;
    let edit_range = match target {
        SemanticEditTarget::Body => locator
            .body_range
            .clone()
            .ok_or_else(|| "code entity has no exact semantic body range".to_owned())?,
        SemanticEditTarget::BeforeEntity => CodeSourceRange {
            start: locator.range.start.clone(),
            end: locator.range.start.clone(),
        },
        SemanticEditTarget::AfterEntity => CodeSourceRange {
            start: locator.range.end.clone(),
            end: locator.range.end.clone(),
        },
        SemanticEditTarget::Entity => locator.range.clone(),
    };
    let intent_identity = semantic_edit_intent_identity(target, &replacement);
    let validation = read_edit_validation(context, repository_id, entity_id, &operation_id)?
        .ok_or_else(|| {
            "semantic edit requires provider syntax/structure validation before mutation".to_owned()
        })?;
    if validation.revision != revision
        || validation.entity != locator.entity
        || validation.intent_identity != intent_identity
        || validation.provider_id != locator.provider_id
        || validation.provider_epoch != locator.provider_epoch
    {
        return Err("semantic edit validation does not match the current edit intent".into());
    }
    if locator.document.provenance != DocumentProvenance::WorkspaceBacked {
        return Err("semantic edit requires workspace-backed provenance".into());
    }
    let expected_revision = locator
        .document
        .file_version
        .as_deref()
        .ok_or_else(|| "semantic edit requires an exact workspace revision".to_owned())?;

    let input = context
        .kernel
        .encode_value(&WorkspaceCommand::Read {
            path: locator.document.path.clone(),
        })
        .map_err(|error| error.to_string())?;
    let output = context
        .kernel
        .invoke_service_abi(&workspace_service(), &input, context.call.authority, None)
        .map_err(|error| error.to_string())?;
    let response = context
        .kernel
        .decode_projected::<WorkspaceResponse>(&WorkspaceInterface::interface_id(), &output)
        .map_err(|error| error.to_string())?;
    let WorkspaceResponse::Read {
        path,
        content: source,
        version,
    } = response
    else {
        return Err("workspace returned a non-read response while preparing semantic edit".into());
    };
    if path != locator.document.path {
        return Err(format!(
            "semantic edit path mismatch: expected {}, observed {path}",
            locator.document.path
        ));
    }
    let WorkspaceFileVersion::Present { content_hash } = &version else {
        return Err(format!("semantic edit source path is absent: {path}"));
    };
    if !workspace_revision_matches(content_hash, expected_revision) {
        return Err(format!(
            "semantic edit source revision is stale: expected {expected_revision}, current {}",
            workspace_revision_label(content_hash)
        ));
    }

    let start = source_position_offset(&source, &edit_range.start, locator.position_encoding)?;
    let end = source_position_offset(&source, &edit_range.end, locator.position_encoding)?;
    if end < start {
        return Err("semantic edit range end precedes start".into());
    }
    let before = source
        .get(start..end)
        .ok_or_else(|| "semantic edit range is not on UTF-8 boundaries".to_owned())?
        .to_owned();
    let before_content_identity = format!("sha256:{:x}", Sha256::digest(source.as_bytes()));
    let mut updated = String::with_capacity(
        source
            .len()
            .saturating_sub(end.saturating_sub(start))
            .saturating_add(replacement.len()),
    );
    updated.push_str(
        source
            .get(..start)
            .ok_or_else(|| "semantic edit range start is not on a UTF-8 boundary".to_owned())?,
    );
    updated.push_str(&replacement);
    updated.push_str(
        source
            .get(end..)
            .ok_or_else(|| "semantic edit range end is not on a UTF-8 boundary".to_owned())?,
    );

    let after_content_identity = format!("sha256:{:x}", Sha256::digest(updated.as_bytes()));
    let evidence = CodeEntityEditEvidence {
        range: edit_range,
        before,
        after: replacement,
        before_content_identity,
        after_content_identity,
    };

    let input = context
        .kernel
        .encode_value(&WorkspaceCommand::CommitBatch {
            operation_id,
            writes: vec![WorkspaceWrite {
                path: locator.document.path.clone(),
                content: updated,
                expected_version: version,
            }],
        })
        .map_err(|error| error.to_string())?;
    let output = context
        .kernel
        .invoke_service_abi(&workspace_service(), &input, context.call.authority, None)
        .map_err(|error| error.to_string())?;
    let response = context
        .kernel
        .decode_projected::<WorkspaceResponse>(&WorkspaceInterface::interface_id(), &output)
        .map_err(|error| error.to_string())?;
    let receipt = match response {
        WorkspaceResponse::CommittedBatch { receipt } => receipt,
        WorkspaceResponse::VersionConflict { conflicts } => {
            return Err(format!(
                "semantic edit source became stale before commit: {conflicts:?}"
            ))
        }
        WorkspaceResponse::UnsupportedAtomicScope {
            requested,
            available,
        } => {
            return Err(format!(
            "semantic edit requires {requested:?} workspace writes; backend provides {available:?}"
        ))
        }
        other => {
            return Err(format!(
                "workspace returned an unexpected semantic edit response: {other:?}"
            ))
        }
    };

    Ok(CodeEntityEditResult {
        entity: locator.entity,
        source_revision: locator.revision,
        document: locator.document,
        receipt,
        validation,
        evidence,
    })
}

fn read_entity_range(
    context: &LanguageContext<'_, '_, '_>,
    locator: CodeEntitySourceLocator,
    range: CodeSourceRange,
    max_bytes: u64,
    label: &str,
) -> Result<Option<CodeEntitySourceView>, String> {
    if max_bytes == 0 {
        return Err(format!("{label} read requires a non-zero byte bound"));
    }
    if locator.document.provenance != DocumentProvenance::WorkspaceBacked {
        return Err(format!("{label} read requires workspace-backed provenance"));
    }
    let expected_version = locator
        .document
        .file_version
        .as_deref()
        .ok_or_else(|| format!("{label} read requires an exact workspace revision"))?;

    let input = context
        .kernel
        .encode_value(&WorkspaceCommand::Read {
            path: locator.document.path.clone(),
        })
        .map_err(|error| error.to_string())?;
    let output = context
        .kernel
        .invoke_service_abi(&workspace_service(), &input, context.call.authority, None)
        .map_err(|error| error.to_string())?;
    let response = context
        .kernel
        .decode_projected::<WorkspaceResponse>(&WorkspaceInterface::interface_id(), &output)
        .map_err(|error| error.to_string())?;
    let WorkspaceResponse::Read {
        path,
        content,
        version,
    } = response
    else {
        return Err(format!(
            "workspace returned a non-read response for {label}"
        ));
    };
    if path != locator.document.path {
        return Err(format!(
            "{label} path mismatch: expected {}, observed {path}",
            locator.document.path
        ));
    }
    let WorkspaceFileVersion::Present { content_hash } = version else {
        return Err(format!("{label} path is absent: {path}"));
    };
    if !workspace_revision_matches(&content_hash, expected_version) {
        return Err(format!(
            "{label} revision is stale: expected {expected_version}, current {}",
            workspace_revision_label(&content_hash)
        ));
    }

    let source = source_range_slice(&content, &range, locator.position_encoding)?;
    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let (content, complete) = bounded_utf8(source, max_bytes);
    Ok(Some(CodeEntitySourceView {
        entity: locator.entity,
        revision: locator.revision,
        document: locator.document,
        position_encoding: locator.position_encoding,
        range,
        content,
        complete,
    }))
}

fn validate_code_source_range(range: &CodeSourceRange, label: &str) -> Result<(), String> {
    if code_position_key(&range.start) > code_position_key(&range.end) {
        return Err(format!("{label} end precedes start"));
    }
    Ok(())
}

fn code_range_contains(outer: &CodeSourceRange, inner: &CodeSourceRange) -> bool {
    code_position_key(&outer.start) <= code_position_key(&inner.start)
        && code_position_key(&inner.end) <= code_position_key(&outer.end)
}

fn code_position_key(position: &CodeSourcePosition) -> (u32, u32) {
    (position.line, position.character)
}

fn source_range_slice<'source>(
    source: &'source str,
    range: &CodeSourceRange,
    encoding: CodePositionEncoding,
) -> Result<&'source str, String> {
    let start = source_position_offset(source, &range.start, encoding)?;
    let end = source_position_offset(source, &range.end, encoding)?;
    if end < start {
        return Err("entity source range end precedes start".into());
    }
    source
        .get(start..end)
        .ok_or_else(|| "entity source range is not on UTF-8 boundaries".to_owned())
}

fn source_position_offset(
    source: &str,
    position: &CodeSourcePosition,
    encoding: CodePositionEncoding,
) -> Result<usize, String> {
    let (line_start, line_end) = source_line_bounds(source, position.line)?;
    let line = &source[line_start..line_end];
    let character = usize::try_from(position.character)
        .map_err(|_| "source character offset cannot be represented".to_owned())?;
    let within_line = match encoding {
        CodePositionEncoding::Utf8 => {
            if character > line.len() || !line.is_char_boundary(character) {
                return Err("UTF-8 source position is not on a character boundary".into());
            }
            character
        }
        CodePositionEncoding::Utf16 => {
            let mut units = 0usize;
            let mut result = None;
            for (byte, value) in line.char_indices() {
                if units == character {
                    result = Some(byte);
                    break;
                }
                units = units.saturating_add(value.len_utf16());
                if units > character {
                    return Err("UTF-16 source position splits a scalar value".into());
                }
            }
            if result.is_none() && units == character {
                result = Some(line.len());
            }
            result.ok_or_else(|| "UTF-16 source position exceeds line length".to_owned())?
        }
        CodePositionEncoding::Utf32 => {
            if character == 0 {
                0
            } else {
                line.char_indices()
                    .nth(character)
                    .map(|(byte, _)| byte)
                    .or_else(|| (line.chars().count() == character).then_some(line.len()))
                    .ok_or_else(|| "UTF-32 source position exceeds line length".to_owned())?
            }
        }
    };
    Ok(line_start + within_line)
}

fn source_line_bounds(source: &str, target_line: u32) -> Result<(usize, usize), String> {
    let target_line =
        usize::try_from(target_line).map_err(|_| "source line cannot be represented".to_owned())?;
    let bytes = source.as_bytes();
    let mut line = 0usize;
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        if line == target_line {
            let end = if index > start && bytes[index - 1] == b'\r' {
                index - 1
            } else {
                index
            };
            return Ok((start, end));
        }
        line = line.saturating_add(1);
        start = index.saturating_add(1);
    }
    if line == target_line {
        return Ok((start, source.len()));
    }
    Err(format!("source line {target_line} is out of range"))
}

fn bounded_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_owned(), true);
    }
    let mut end = max_bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), false)
}

fn validate_lsp_range(range: &LspRange) -> Result<(), String> {
    if position_key(&range.start) > position_key(&range.end) {
        return Err("LSP document symbol range end precedes start".into());
    }
    Ok(())
}

fn range_contains(outer: &LspRange, inner: &LspRange) -> bool {
    position_key(&outer.start) <= position_key(&inner.start)
        && position_key(&inner.end) <= position_key(&outer.end)
}

fn position_key(position: &LspPosition) -> (u32, u32) {
    (position.line, position.character)
}

fn lsp_range_identity(range: &LspRange) -> String {
    format!(
        "{}:{}-{}:{}",
        range.start.line, range.start.character, range.end.line, range.end.character
    )
}

fn digest_identity(label: &str, parts: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(label.as_bytes());
    for part in parts {
        hasher.update([0]);
        hasher.update(part.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn verify_workspace_document_revision(
    context: &LanguageContext<'_, '_, '_>,
    path: &str,
    expected_version: &str,
) -> Result<(), String> {
    let input = context
        .kernel
        .encode_value(&WorkspaceCommand::Read {
            path: path.to_owned(),
        })
        .map_err(|error| error.to_string())?;
    let output = context
        .kernel
        .invoke_service_abi(&workspace_service(), &input, context.call.authority, None)
        .map_err(|error| error.to_string())?;
    let response = context
        .kernel
        .decode_projected::<WorkspaceResponse>(&WorkspaceInterface::interface_id(), &output)
        .map_err(|error| error.to_string())?;
    let WorkspaceResponse::Read { version, .. } = response else {
        return Err("workspace returned a non-read response while validating provider fact".into());
    };
    let WorkspaceFileVersion::Present { content_hash } = version else {
        return Err(format!("provider fact source path is absent: {path}"));
    };
    if !workspace_revision_matches(&content_hash, expected_version) {
        return Err(format!(
            "provider fact source revision is stale: expected {expected_version}, current {}",
            workspace_revision_label(&content_hash)
        ));
    }
    Ok(())
}

fn workspace_revision_label(content_hash: &str) -> String {
    if content_hash.starts_with("sha256:") {
        content_hash.to_owned()
    } else {
        format!("sha256:{content_hash}")
    }
}

fn workspace_revision_matches(content_hash: &str, expected_version: &str) -> bool {
    expected_version == content_hash
        || expected_version
            .strip_prefix("sha256:")
            .is_some_and(|expected_hash| expected_hash == content_hash)
}

fn store_entity_revision(
    context: &LanguageContext<'_, '_, '_>,
    revision: &CodeEntityRevision,
) -> Result<(), String> {
    let repository_id = revision.entity.repository_id.as_str();
    let history_key = entity_revision_key(repository_id, &revision.entity.id, &revision.revision);
    let current_key = entity_current_key(repository_id, &revision.entity.id);
    let sequence_key = entity_change_sequence_key(repository_id);
    let encoded = serde_json::to_vec(revision).map_err(|error| error.to_string())?;

    if revision.sequence == 0 {
        return Err("code entity revision sequence must be non-zero".into());
    }

    if let Some(existing) = context
        .kernel
        .read_durable(&language_namespace(), &history_key)
        .map_err(|error| error.to_string())?
    {
        let existing: CodeEntityRevision =
            serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
        if existing != *revision {
            return Err(format!(
                "logical entity revision {} already exists with different content",
                revision.revision
            ));
        }
        return Ok(());
    }

    let current = context
        .kernel
        .read_durable(&language_namespace(), &current_key)
        .map_err(|error| error.to_string())?;
    let current_revision = current
        .as_deref()
        .map(|bytes| {
            serde_json::from_slice::<CodeEntityRevision>(bytes).map_err(|error| error.to_string())
        })
        .transpose()?;
    if let Some(current_revision) = &current_revision {
        if revision.sequence <= current_revision.sequence {
            return Err(format!(
                "code entity revision sequence {} must advance beyond current sequence {}",
                revision.sequence, current_revision.sequence
            ));
        }
    }

    let sequence_bytes = context
        .kernel
        .read_durable(&language_namespace(), &sequence_key)
        .map_err(|error| error.to_string())?;
    let current_sequence = sequence_bytes
        .as_deref()
        .map(|bytes| serde_json::from_slice::<u64>(bytes).map_err(|error| error.to_string()))
        .transpose()?
        .unwrap_or(0);
    let change_sequence = current_sequence
        .checked_add(1)
        .ok_or_else(|| "code entity change sequence overflow".to_owned())?;
    let changes = current_revision
        .as_ref()
        .map(|previous| CodeEntityFacetChanges::between(&previous.facets, &revision.facets))
        .unwrap_or_else(|| initial_entity_changes(revision));
    let event = CodeEntityChangeEvent {
        sequence: change_sequence,
        entity: revision.entity.clone(),
        previous_revision: current_revision
            .as_ref()
            .map(|previous| previous.revision.clone()),
        revision: revision.revision.clone(),
        changes,
    };
    let event_key = entity_change_key(repository_id, change_sequence);
    let event_bytes = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
    let next_sequence_bytes =
        serde_json::to_vec(&change_sequence).map_err(|error| error.to_string())?;

    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: history_key.clone(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: current_key.clone(),
                    expected: current,
                },
                TransactionOp::AssertValue {
                    key: sequence_key.clone(),
                    expected: sequence_bytes,
                },
                TransactionOp::AssertValue {
                    key: event_key.clone(),
                    expected: None,
                },
                TransactionOp::Put {
                    key: history_key,
                    value: encoded.clone(),
                },
                TransactionOp::Put {
                    key: current_key,
                    value: encoded,
                },
                TransactionOp::Put {
                    key: event_key,
                    value: event_bytes,
                },
                TransactionOp::Put {
                    key: sequence_key,
                    value: next_sequence_bytes,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn initial_entity_changes(revision: &CodeEntityRevision) -> CodeEntityFacetChanges {
    CodeEntityFacetChanges {
        existence: true,
        name_location: true,
        signature: revision.facets.signature.is_some(),
        body: revision.facets.body.is_some(),
        relations: revision.facets.relations.keys().cloned().collect(),
    }
}

fn read_entity_changes(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    after_sequence: u64,
    limit: u32,
) -> Result<CodeEntityChangePage, String> {
    if !(1..=100).contains(&limit) {
        return Err("code entity change page limit must be between 1 and 100".into());
    }
    let current_sequence = read_entity_change_sequence(context, repository_id)?;
    if after_sequence > current_sequence {
        return Err(format!(
            "code entity change cursor {after_sequence} is ahead of current sequence {current_sequence}"
        ));
    }

    let mut events = Vec::new();
    let mut sequence = after_sequence.saturating_add(1);
    while sequence <= current_sequence && events.len() < limit as usize {
        let bytes = context
            .kernel
            .read_durable(
                &language_namespace(),
                &entity_change_key(repository_id, sequence),
            )
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("code entity change stream gap at sequence {sequence}"))?;
        let event: CodeEntityChangeEvent =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if event.sequence != sequence || event.entity.repository_id != repository_id {
            return Err(format!(
                "invalid code entity change event at sequence {sequence}"
            ));
        }
        events.push(event);
        sequence = sequence.saturating_add(1);
    }
    let next_after_sequence = events
        .last()
        .map(|event| event.sequence)
        .unwrap_or(after_sequence);
    Ok(CodeEntityChangePage {
        repository_id: repository_id.to_owned(),
        after_sequence,
        current_sequence,
        events,
        next_after_sequence,
        caught_up: next_after_sequence >= current_sequence,
    })
}

fn store_entity_lineage(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    lineage: &CodeEntityLineage,
) -> Result<(), String> {
    let key = entity_lineage_key(repository_id, lineage);
    let encoded = serde_json::to_vec(lineage).map_err(|error| error.to_string())?;
    if let Some(existing) = context
        .kernel
        .read_durable(&language_namespace(), &key)
        .map_err(|error| error.to_string())?
    {
        let existing: CodeEntityLineage =
            serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
        if existing == *lineage {
            return Ok(());
        }
        return Err("code entity lineage already exists with different evidence".into());
    }
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::Put {
                    key,
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_entity_lineage(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    from_entity_id: &str,
    to_entity_id: &str,
    kind: CodeEntityLineageKind,
) -> Result<Option<CodeEntityLineage>, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &entity_lineage_key_parts(repository_id, from_entity_id, to_entity_id, kind),
        )
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_entity_revision(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
) -> Result<Option<CodeEntityRevision>, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &entity_current_key(repository_id, entity_id),
        )
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_entity_revision_version(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    entity_id: &str,
    revision: &str,
) -> Result<Option<CodeEntityRevision>, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &entity_revision_key(repository_id, entity_id, revision),
        )
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_entity_change_sequence(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
) -> Result<u64, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &entity_change_sequence_key(repository_id),
        )
        .map_err(|error| error.to_string())?
        .as_deref()
        .map(|bytes| serde_json::from_slice::<u64>(bytes).map_err(|error| error.to_string()))
        .transpose()
        .map(|sequence| sequence.unwrap_or(0))
}

fn begin_identity_rebuild(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
) -> Result<CodeIdentityRebuildCheckpoint, String> {
    let state_key = identity_continuity_key(repository_id);
    let checkpoint_key = identity_rebuild_key(repository_id);
    let current_state = context
        .kernel
        .read_durable(&language_namespace(), &state_key)
        .map_err(|error| error.to_string())?;
    let current_checkpoint = context
        .kernel
        .read_durable(&language_namespace(), &checkpoint_key)
        .map_err(|error| error.to_string())?;

    if let (Some(state_bytes), Some(checkpoint_bytes)) =
        (current_state.as_deref(), current_checkpoint.as_deref())
    {
        let state: CodeIdentityContinuityState =
            serde_json::from_slice(state_bytes).map_err(|error| error.to_string())?;
        let checkpoint: CodeIdentityRebuildCheckpoint =
            serde_json::from_slice(checkpoint_bytes).map_err(|error| error.to_string())?;
        if state.status == CodeIdentityContinuityStatus::Rebuilding {
            return Ok(checkpoint);
        }
    }

    let checkpoint = CodeIdentityRebuildCheckpoint {
        repository_id: repository_id.to_owned(),
        required_through_sequence: read_entity_change_sequence(context, repository_id)?,
    };
    let state = CodeIdentityContinuityState {
        repository_id: repository_id.to_owned(),
        status: CodeIdentityContinuityStatus::Rebuilding,
        reason: None,
    };
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: state_key.clone(),
                    expected: current_state,
                },
                TransactionOp::AssertValue {
                    key: checkpoint_key.clone(),
                    expected: current_checkpoint,
                },
                TransactionOp::Put {
                    key: state_key,
                    value: serde_json::to_vec(&state).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: checkpoint_key,
                    value: serde_json::to_vec(&checkpoint).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(checkpoint)
}

fn complete_identity_rebuild(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
    applied_through_sequence: u64,
) -> Result<CodeIdentityRebuildCheckpoint, String> {
    let state_key = identity_continuity_key(repository_id);
    let checkpoint_key = identity_rebuild_key(repository_id);
    let state_bytes = context
        .kernel
        .read_durable(&language_namespace(), &state_key)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "identity rebuild is not active".to_owned())?;
    let checkpoint_bytes = context
        .kernel
        .read_durable(&language_namespace(), &checkpoint_key)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "identity rebuild checkpoint is missing".to_owned())?;
    let state: CodeIdentityContinuityState =
        serde_json::from_slice(&state_bytes).map_err(|error| error.to_string())?;
    if state.status != CodeIdentityContinuityStatus::Rebuilding {
        return Err("identity rebuild is not active".into());
    }
    let checkpoint: CodeIdentityRebuildCheckpoint =
        serde_json::from_slice(&checkpoint_bytes).map_err(|error| error.to_string())?;
    if checkpoint.repository_id != repository_id {
        return Err("identity rebuild checkpoint repository mismatch".into());
    }

    let current_sequence = read_entity_change_sequence(context, repository_id)?;
    if applied_through_sequence != current_sequence {
        return Err(format!(
            "identity rebuild is not caught up: applied through {applied_through_sequence}, current sequence is {current_sequence}"
        ));
    }

    let available = CodeIdentityContinuityState {
        repository_id: repository_id.to_owned(),
        status: CodeIdentityContinuityStatus::Available,
        reason: None,
    };
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: state_key.clone(),
                    expected: Some(state_bytes),
                },
                TransactionOp::AssertValue {
                    key: checkpoint_key.clone(),
                    expected: Some(checkpoint_bytes),
                },
                TransactionOp::Put {
                    key: state_key,
                    value: serde_json::to_vec(&available).map_err(|error| error.to_string())?,
                },
                TransactionOp::Delete {
                    key: checkpoint_key,
                },
            ],
        )
        .map_err(|error| error.to_string())?;

    Ok(CodeIdentityRebuildCheckpoint {
        repository_id: repository_id.to_owned(),
        required_through_sequence: current_sequence,
    })
}

fn store_identity_continuity(
    context: &LanguageContext<'_, '_, '_>,
    state: &CodeIdentityContinuityState,
) -> Result<(), String> {
    let encoded = serde_json::to_vec(state).map_err(|error| error.to_string())?;
    context
        .kernel
        .transact_durable(
            &language_namespace(),
            &[TransactionOp::Put {
                key: identity_continuity_key(&state.repository_id),
                value: encoded,
            }],
        )
        .map_err(|error| error.to_string())
}

fn read_identity_continuity(
    context: &LanguageContext<'_, '_, '_>,
    repository_id: &str,
) -> Result<Option<CodeIdentityContinuityState>, String> {
    context
        .kernel
        .read_durable(
            &language_namespace(),
            &identity_continuity_key(repository_id),
        )
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn validate_code_entity_lineage(
    repository_id: &str,
    lineage: &CodeEntityLineage,
) -> Result<(), String> {
    validate_identity("code repository id", repository_id)?;
    validate_identity("lineage source entity id", &lineage.from_entity_id)?;
    validate_identity("lineage target entity id", &lineage.to_entity_id)?;
    for observation_id in &lineage.evidence_observation_ids {
        validate_identity("lineage evidence observation id", observation_id)?;
    }
    if matches!(
        lineage.kind,
        CodeEntityLineageKind::Rename | CodeEntityLineageKind::Move
    ) && lineage.confidence == CodeEntityLineageConfidence::Confirmed
        && lineage.from_entity_id != lineage.to_entity_id
    {
        return Err("confirmed rename/move lineage must preserve logical entity identity".into());
    }
    if matches!(
        lineage.kind,
        CodeEntityLineageKind::Replacement
            | CodeEntityLineageKind::Extract
            | CodeEntityLineageKind::Split
            | CodeEntityLineageKind::Merge
    ) && lineage.from_entity_id == lineage.to_entity_id
    {
        return Err(
            "replacement/extract/split/merge lineage requires a distinct target identity".into(),
        );
    }
    Ok(())
}

fn validate_code_entity_revision(revision: &CodeEntityRevision) -> Result<(), String> {
    validate_identity("logical code entity id", &revision.entity.id)?;
    validate_identity("code repository id", &revision.entity.repository_id)?;
    validate_identity("code entity revision", &revision.revision)?;
    validate_identity("code entity name", &revision.name)?;
    validate_identity("language provider id", &revision.provider_id)?;
    validate_documents(std::slice::from_ref(&revision.document))?;
    for (label, value) in [
        (
            "existence facet revision",
            revision.facets.existence.as_str(),
        ),
        (
            "name/location facet revision",
            revision.facets.name_location.as_str(),
        ),
    ] {
        validate_identity(label, value)?;
    }
    for (label, value) in [
        ("signature identity", revision.signature_identity.as_deref()),
        ("body identity", revision.body_identity.as_deref()),
        (
            "signature facet revision",
            revision.facets.signature.as_deref(),
        ),
        ("body facet revision", revision.facets.body.as_deref()),
    ] {
        if let Some(value) = value {
            validate_identity(label, value)?;
        }
    }
    for (relation, facet_revision) in &revision.facets.relations {
        validate_identity("relation facet", relation)?;
        validate_identity("relation facet revision", facet_revision)?;
    }
    Ok(())
}

fn validate_documents(documents: &[LanguageDocumentIdentity]) -> Result<(), String> {
    for document in documents {
        validate_identity("language document path", &document.path)?;
        if matches!(document.provenance, DocumentProvenance::WorkspaceBacked)
            && document.file_version.as_deref().is_none_or(str::is_empty)
        {
            return Err("workspace-backed language evidence requires an exact file version".into());
        }
    }
    Ok(())
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(())
    }
}

fn observation_key(id: &str) -> String {
    format!("observation/{id}")
}

fn identity_continuity_key(repository_id: &str) -> String {
    format!("entity/{repository_id}/continuity")
}

fn identity_rebuild_key(repository_id: &str) -> String {
    format!("entity/{repository_id}/continuity/rebuild")
}

fn lineage_kind_key(kind: CodeEntityLineageKind) -> &'static str {
    match kind {
        CodeEntityLineageKind::Rename => "rename",
        CodeEntityLineageKind::Move => "move",
        CodeEntityLineageKind::Replacement => "replacement",
        CodeEntityLineageKind::Extract => "extract",
        CodeEntityLineageKind::Split => "split",
        CodeEntityLineageKind::Merge => "merge",
    }
}

fn entity_lineage_key(repository_id: &str, lineage: &CodeEntityLineage) -> String {
    entity_lineage_key_parts(
        repository_id,
        &lineage.from_entity_id,
        &lineage.to_entity_id,
        lineage.kind,
    )
}

fn entity_lineage_key_parts(
    repository_id: &str,
    from_entity_id: &str,
    to_entity_id: &str,
    kind: CodeEntityLineageKind,
) -> String {
    format!(
        "entity/{repository_id}/lineage/{from_entity_id}/{to_entity_id}/{}",
        lineage_kind_key(kind)
    )
}

fn entity_change_sequence_key(repository_id: &str) -> String {
    format!("entity/{repository_id}/changes/@sequence")
}

fn entity_change_key(repository_id: &str, sequence: u64) -> String {
    format!("entity/{repository_id}/changes/{sequence:020}")
}

fn entity_current_key(repository_id: &str, entity_id: &str) -> String {
    format!("entity/{repository_id}/{entity_id}/current")
}

fn entity_revision_key(repository_id: &str, entity_id: &str, revision: &str) -> String {
    format!("entity/{repository_id}/{entity_id}/revision/{revision}")
}

fn entity_source_locator_key(repository_id: &str, entity_id: &str, revision: &str) -> String {
    format!("entity/{repository_id}/{entity_id}/source/{revision}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        Kernel, KernelConfig, LocalPersistence, PhenixValue, Project, ResolvedHarness,
        ResolvedHarnessActivation,
    };
    use phenix_plugin_environment_local::{
        local_environment_component_manifest, local_environment_factory_for,
        local_environment_manifest,
    };
    use phenix_sdk::{
        CodeEntityChangePage, CodeEntityFacetChanges, CodeEntityFacetRevisions, CodeEntityLineage,
        CodeEntityLineageConfidence, CodeEntityLineageKind, CodeIdentityContinuityState,
        CodeIdentityContinuityStatus, LanguageOperationKind, LanguageOperationResult,
        LogicalCodeEntity,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_db(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "phenix-{name}-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }

    fn kernel_with(path: &PathBuf) -> Kernel {
        let manifest = language_manifest();
        let plugin = manifest.id.clone();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel =
            Kernel::with_persistence(KernelConfig::new([manifest]).unwrap(), persistence);
        kernel
            .register_embedded_factory(plugin, language_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn kernel_with_workspace(path: &PathBuf, root: &Path) -> Kernel {
        let language = language_manifest();
        let language_id = language.id.clone();
        let environment = local_environment_manifest();
        let environment_id = environment.id.clone();
        let workspace = phenix_plugin_workspace::workspace_manifest();
        let workspace_id = workspace.id.clone();
        let authority = Authority::new(
            language
                .maximum_authority
                .capabilities()
                .chain(environment.maximum_authority.capabilities())
                .chain(workspace.maximum_authority.capabilities())
                .cloned(),
        );
        let resolved = ResolvedHarness::resolve(
            [language.clone(), environment.clone(), workspace.clone()],
            [
                local_environment_component_manifest(),
                phenix_plugin_workspace::workspace_component_manifest(),
            ],
            [],
            &authority,
        )
        .unwrap();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel = Kernel::with_persistence(
            KernelConfig::new([language, environment, workspace]).unwrap(),
            persistence,
        );
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(language_id, language_factory)
            .unwrap();
        let environment_root = root.to_path_buf();
        kernel
            .register_embedded_factory(environment_id, move || {
                local_environment_factory_for(environment_root.clone())
            })
            .unwrap();
        let workspace_root = root.to_path_buf();
        kernel
            .register_embedded_factory(workspace_id, move || {
                phenix_plugin_workspace::workspace_factory_for(workspace_root.clone())
            })
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke(kernel: &mut Kernel, command: LanguageCommand) -> Result<LanguageResponse, String> {
        let input = serde_json::to_vec(&phenix_core::PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(
                &language_service(),
                &input,
                &language_manifest().maximum_authority,
                None,
            )
            .map_err(|error| error.to_string())?;
        let output: phenix_core::PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        output.project().map_err(|error| error.to_string())
    }

    fn invoke_workspace(
        kernel: &mut Kernel,
        command: WorkspaceCommand,
    ) -> Result<WorkspaceResponse, String> {
        let input = serde_json::to_vec(&phenix_core::PhenixValue::from(&command)).unwrap();
        let output = kernel
            .invoke(
                &workspace_service(),
                &input,
                &phenix_plugin_workspace::workspace_manifest().maximum_authority,
                None,
            )
            .map_err(|error| error.to_string())?;
        let output: phenix_core::PhenixValue =
            serde_json::from_slice(&output).map_err(|error| error.to_string())?;
        output.project().map_err(|error| error.to_string())
    }

    fn epoch(value: u64) -> ProviderEpoch {
        ProviderEpoch::new(value).unwrap()
    }

    fn activate(kernel: &mut Kernel, value: u64) {
        invoke(
            kernel,
            LanguageCommand::ActivateProvider {
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(value),
            },
        )
        .unwrap();
    }

    fn validate_semantic_edit(
        kernel: &mut Kernel,
        revision: &CodeEntityRevision,
        operation_id: &str,
        target: SemanticEditTarget,
        replacement: &str,
        observation_id: &str,
    ) {
        let fact_id = format!("validation-{operation_id}");
        invoke(
            kernel,
            LanguageCommand::Consume {
                observation_id: observation_id.into(),
                execution_id: format!("execution-{observation_id}"),
                workspace_id: "workspace".into(),
                provider_id: revision.provider_id.clone(),
                epoch: revision.provider_epoch,
                result: LanguageOperationResult {
                    operation: LanguageOperationKind::EditValidation,
                    payload: serde_json::json!({
                        "facts": [{
                            "id": fact_id,
                            "operation_id": operation_id,
                            "entity": {
                                "id": revision.entity.id,
                                "repository_id": revision.entity.repository_id
                            },
                            "revision": revision.revision,
                            "intent_identity": semantic_edit_intent_identity(target, replacement),
                            "valid": true
                        }]
                    })
                    .into(),
                    documents: vec![revision.document.clone()],
                },
            },
        )
        .unwrap();

        let response = invoke(
            kernel,
            LanguageCommand::IngestEditValidation {
                observation_id: observation_id.into(),
                fact_id,
            },
        )
        .unwrap();
        let LanguageResponse::EditValidation {
            validation: Some(validation),
        } = response
        else {
            panic!("expected semantic edit validation");
        };
        assert_eq!(validation.operation_id, operation_id);
        assert_eq!(validation.entity, revision.entity);
        assert_eq!(validation.revision, revision.revision);
    }

    fn workspace_result(operation: LanguageOperationKind) -> LanguageOperationResult {
        LanguageOperationResult {
            operation,
            payload: serde_json::json!({"items": ["result"]}).into(),
            documents: vec![LanguageDocumentIdentity {
                path: "src/lib.rs".into(),
                file_version: Some("sha256:abc".into()),
                provenance: DocumentProvenance::WorkspaceBacked,
            }],
        }
    }

    fn diagnostics_result() -> DiagnosticsResult {
        DiagnosticsResult::Diagnostics {
            payload: serde_json::json!({"items": ["result"]}).into(),
            documents: vec![LanguageDocumentIdentity {
                path: "src/lib.rs".into(),
                file_version: Some("sha256:abc".into()),
                provenance: DocumentProvenance::WorkspaceBacked,
            }],
        }
    }

    #[test]
    fn providerless_fallback_reads_exact_workspace_revision_without_semantic_claims() {
        let path = temp_db("file-fallback");
        let root = std::env::temp_dir().join(format!(
            "phenix-language-file-fallback-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn fallback() {}\n").unwrap();
        let mut kernel = kernel_with_workspace(&path, &root);

        let response = invoke(
            &mut kernel,
            LanguageCommand::ReadFileFallback {
                workspace_id: "workspace".into(),
                path: "src/lib.rs".into(),
            },
        )
        .unwrap();
        let LanguageResponse::FileFallback { fallback } = response else {
            panic!("expected exact file fallback");
        };
        assert_eq!(fallback.workspace_id, "workspace");
        assert_eq!(fallback.document.path, "src/lib.rs");
        assert_eq!(
            fallback.document.provenance,
            DocumentProvenance::WorkspaceBacked
        );
        assert!(fallback
            .document
            .file_version
            .as_deref()
            .is_some_and(|revision| {
                revision.starts_with("sha256:") && revision.len() > "sha256:".len()
            }));
        assert_eq!(fallback.content, "fn fallback() {}\n");

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn entity_source_read_is_revision_checked_bounded_and_encoding_aware() {
        let path = temp_db("entity-source-read");
        let root = std::env::temp_dir().join(format!(
            "phenix-language-entity-source-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn café() {}\n").unwrap();
        let mut kernel = kernel_with_workspace(&path, &root);

        let LanguageResponse::FileFallback { fallback } = invoke(
            &mut kernel,
            LanguageCommand::ReadFileFallback {
                workspace_id: "workspace".into(),
                path: "src/lib.rs".into(),
            },
        )
        .unwrap() else {
            panic!("expected exact workspace fallback");
        };

        activate(&mut kernel, 9);
        invoke(
            &mut kernel,
            LanguageCommand::Consume {
                observation_id: "symbols-source".into(),
                execution_id: "execution-source".into(),
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(9),
                result: LanguageOperationResult {
                    operation: LanguageOperationKind::DocumentSymbols,
                    payload: serde_json::json!([{
                        "name": "café",
                        "kind": 12,
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 12}
                        },
                        "selectionRange": {
                            "start": {"line": 0, "character": 3},
                            "end": {"line": 0, "character": 7}
                        }
                    }])
                    .into(),
                    documents: vec![fallback.document],
                },
            },
        )
        .unwrap();

        let LanguageResponse::EntityRevisions { revisions } = invoke(
            &mut kernel,
            LanguageCommand::IngestDocumentSymbolsWithEncoding {
                observation_id: "symbols-source".into(),
                repository_id: "repo-source".into(),
                position_encoding: CodePositionEncoding::Utf16,
            },
        )
        .unwrap() else {
            panic!("expected entity revisions");
        };
        let revision = &revisions[0];

        let LanguageResponse::EntitySource { view: Some(view) } = invoke(
            &mut kernel,
            LanguageCommand::ReadEntitySource {
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                max_bytes: 8,
            },
        )
        .unwrap() else {
            panic!("expected bounded entity source");
        };
        assert_eq!(view.content, "fn café");
        assert!(!view.complete);
        assert_eq!(view.position_encoding, CodePositionEncoding::Utf16);

        fs::write(root.join("src/lib.rs"), "fn changed() {}\n").unwrap();
        let error = invoke(
            &mut kernel,
            LanguageCommand::ReadEntitySource {
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                max_bytes: 1024,
            },
        )
        .unwrap_err();
        assert!(error.contains("stale"));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn provider_fact_ingestion_reuses_exact_language_observation_and_rejects_stale_source() {
        let path = temp_db("provider-fact-ingestion");
        let root = std::env::temp_dir().join(format!(
            "phenix-language-provider-fact-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn observed() {}\n").unwrap();
        let mut kernel = kernel_with_workspace(&path, &root);

        let LanguageResponse::FileFallback { fallback } = invoke(
            &mut kernel,
            LanguageCommand::ReadFileFallback {
                workspace_id: "workspace".into(),
                path: "src/lib.rs".into(),
            },
        )
        .unwrap() else {
            panic!("expected exact workspace fallback");
        };

        activate(&mut kernel, 7);
        invoke(
            &mut kernel,
            LanguageCommand::Consume {
                observation_id: "symbols-1".into(),
                execution_id: "execution-1".into(),
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(7),
                result: LanguageOperationResult {
                    operation: LanguageOperationKind::DocumentSymbols,
                    payload: serde_json::json!({
                        "facts": [{
                            "id": "fact-observed",
                            "entity": {
                                "id": "entity-observed",
                                "repository_id": "repo-1"
                            },
                            "revision": "revision-1",
                            "sequence": 1,
                            "document_index": 0,
                            "symbol": "crate::observed",
                            "name": "observed",
                            "signature_identity": "signature-1",
                            "body_identity": "body-1",
                            "source": {
                                "position_encoding": "utf8",
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 0, "character": 16}
                                },
                                "selection_range": {
                                    "start": {"line": 0, "character": 3},
                                    "end": {"line": 0, "character": 11}
                                },
                                "body_range": {
                                    "start": {"line": 0, "character": 14},
                                    "end": {"line": 0, "character": 16}
                                }
                            },
                            "facets": {
                                "existence": "existence-1",
                                "name_location": "location-1",
                                "signature": "signature-1",
                                "body": "body-1",
                                "relations": {"callers": "callers-1"}
                            }
                        }, {
                            "id": "fact-stale",
                            "entity": {
                                "id": "entity-stale",
                                "repository_id": "repo-1"
                            },
                            "revision": "revision-stale",
                            "sequence": 1,
                            "document_index": 0,
                            "symbol": "crate::observed",
                            "name": "observed",
                            "signature_identity": "signature-1",
                            "body_identity": "body-1",
                            "facets": {
                                "existence": "existence-stale",
                                "name_location": "location-stale",
                                "signature": "signature-stale",
                                "body": "body-stale",
                                "relations": {}
                            }
                        }]
                    })
                    .into(),
                    documents: vec![fallback.document.clone()],
                },
            },
        )
        .unwrap();

        let response = invoke(
            &mut kernel,
            LanguageCommand::IngestEntityFact {
                observation_id: "symbols-1".into(),
                fact_id: "fact-observed".into(),
            },
        )
        .unwrap();
        let LanguageResponse::EntityRevision {
            revision: Some(revision),
        } = response
        else {
            panic!("expected ingested entity revision");
        };
        assert_eq!(revision.provider_id, "rust-analyzer");
        assert_eq!(revision.provider_epoch, epoch(7));
        assert_eq!(revision.document, fallback.document);

        let LanguageResponse::EntityBody { view: Some(body) } = invoke(
            &mut kernel,
            LanguageCommand::ReadEntityBody {
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                max_bytes: 1024,
            },
        )
        .unwrap() else {
            panic!("expected provider-backed semantic body");
        };
        assert_eq!(body.content, "{}");
        assert!(body.complete);
        assert_eq!(body.range.start.character, 14);
        assert_eq!(body.range.end.character, 16);

        validate_semantic_edit(
            &mut kernel,
            &revision,
            "replace-observed-body-1",
            SemanticEditTarget::Body,
            "{ 42 }",
            "validate-replace-observed-body-1",
        );

        let LanguageResponse::EntityEdit { result: edit } = invoke(
            &mut kernel,
            LanguageCommand::ReplaceEntityBody {
                operation_id: "replace-observed-body-1".into(),
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                content: "{ 42 }".into(),
            },
        )
        .unwrap() else {
            panic!("expected committed semantic body replacement");
        };
        assert_eq!(edit.entity, revision.entity);
        assert_eq!(edit.source_revision, revision.revision);
        assert_eq!(edit.receipt.operation_id, "replace-observed-body-1");
        assert_eq!(edit.receipt.files.len(), 1);
        let evidence = &edit.receipt.files[0];
        assert_eq!(evidence.path, "src/lib.rs");
        assert_eq!(
            evidence.before_version,
            WorkspaceFileVersion::Present {
                content_hash: format!("{:x}", Sha256::digest(b"fn observed() {}\n")),
            }
        );
        assert_eq!(
            evidence.version,
            WorkspaceFileVersion::Present {
                content_hash: format!("{:x}", Sha256::digest(b"fn observed() { 42 }\n")),
            }
        );
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn observed() { 42 }\n"
        );

        validate_semantic_edit(
            &mut kernel,
            &revision,
            "replace-observed-body-2",
            SemanticEditTarget::Body,
            "{ 99 }",
            "validate-replace-observed-body-2",
        );

        let stale_edit = invoke(
            &mut kernel,
            LanguageCommand::ReplaceEntityBody {
                operation_id: "replace-observed-body-2".into(),
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                content: "{ 99 }".into(),
            },
        )
        .unwrap_err();
        assert!(
            stale_edit.contains("source revision is stale"),
            "unexpected stale semantic edit error: {stale_edit}"
        );
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn observed() { 42 }\n"
        );

        fs::write(root.join("src/lib.rs"), "fn observed() {}\n").unwrap();
        validate_semantic_edit(
            &mut kernel,
            &revision,
            "insert-before-observed-1",
            SemanticEditTarget::BeforeEntity,
            "/* before */\n",
            "validate-insert-before-observed-1",
        );

        let LanguageResponse::EntityEdit { result: before } = invoke(
            &mut kernel,
            LanguageCommand::InsertRelativeToEntity {
                operation_id: "insert-before-observed-1".into(),
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                position: CodeEntityInsertPosition::Before,
                content: "/* before */\n".into(),
            },
        )
        .unwrap() else {
            panic!("expected committed insert-before edit");
        };
        assert_eq!(before.receipt.operation_id, "insert-before-observed-1");
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "/* before */\nfn observed() {}\n"
        );

        fs::write(root.join("src/lib.rs"), "fn observed() {}\n").unwrap();
        validate_semantic_edit(
            &mut kernel,
            &revision,
            "insert-after-observed-1",
            SemanticEditTarget::AfterEntity,
            "/* after */",
            "validate-insert-after-observed-1",
        );

        let LanguageResponse::EntityEdit { result: after } = invoke(
            &mut kernel,
            LanguageCommand::InsertRelativeToEntity {
                operation_id: "insert-after-observed-1".into(),
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
                position: CodeEntityInsertPosition::After,
                content: "/* after */".into(),
            },
        )
        .unwrap() else {
            panic!("expected committed insert-after edit");
        };
        assert_eq!(after.receipt.operation_id, "insert-after-observed-1");
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn observed() {}/* after */\n"
        );

        fs::write(root.join("src/lib.rs"), "fn observed() {}\n").unwrap();
        validate_semantic_edit(
            &mut kernel,
            &revision,
            "remove-observed-1",
            SemanticEditTarget::Entity,
            "",
            "validate-remove-observed-1",
        );

        let LanguageResponse::EntityEdit { result: removed } = invoke(
            &mut kernel,
            LanguageCommand::RemoveEntity {
                operation_id: "remove-observed-1".into(),
                repository_id: revision.entity.repository_id.clone(),
                entity_id: revision.entity.id.clone(),
                revision: revision.revision.clone(),
            },
        )
        .unwrap() else {
            panic!("expected committed entity removal");
        };
        assert_eq!(removed.receipt.operation_id, "remove-observed-1");
        assert_eq!(fs::read_to_string(root.join("src/lib.rs")).unwrap(), "\n");

        fs::write(
            root.join("src/lib.rs"),
            "fn changed_after_observation() {}\n",
        )
        .unwrap();
        let stale = invoke(
            &mut kernel,
            LanguageCommand::IngestEntityFact {
                observation_id: "symbols-1".into(),
                fact_id: "fact-stale".into(),
            },
        )
        .unwrap_err();
        assert!(stale.contains("source revision is stale"));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lsp_document_symbols_ingest_conservative_entity_revisions() {
        let path = temp_db("lsp-document-symbol-ingestion");
        let root = std::env::temp_dir().join(format!(
            "phenix-language-lsp-symbols-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn outer() { fn inner() {} }\n",
        )
        .unwrap();
        let mut kernel = kernel_with_workspace(&path, &root);

        let LanguageResponse::FileFallback { fallback } = invoke(
            &mut kernel,
            LanguageCommand::ReadFileFallback {
                workspace_id: "workspace".into(),
                path: "src/lib.rs".into(),
            },
        )
        .unwrap() else {
            panic!("expected exact workspace fallback");
        };

        activate(&mut kernel, 9);
        invoke(
            &mut kernel,
            LanguageCommand::Consume {
                observation_id: "lsp-symbols-1".into(),
                execution_id: "execution-1".into(),
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(9),
                result: LanguageOperationResult {
                    operation: LanguageOperationKind::DocumentSymbols,
                    payload: serde_json::json!([{
                        "name": "outer",
                        "detail": "fn outer()",
                        "kind": 12,
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 34}
                        },
                        "selectionRange": {
                            "start": {"line": 0, "character": 7},
                            "end": {"line": 0, "character": 12}
                        },
                        "children": [{
                            "name": "inner",
                            "detail": "fn inner()",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 17},
                                "end": {"line": 0, "character": 30}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 20},
                                "end": {"line": 0, "character": 25}
                            }
                        }]
                    }])
                    .into(),
                    documents: vec![fallback.document.clone()],
                },
            },
        )
        .unwrap();

        let LanguageResponse::EntityRevisions { revisions } = invoke(
            &mut kernel,
            LanguageCommand::IngestDocumentSymbols {
                observation_id: "lsp-symbols-1".into(),
                repository_id: "repo-1".into(),
            },
        )
        .unwrap() else {
            panic!("expected normalized entity revisions");
        };
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[0].provider_id, "rust-analyzer");
        assert_eq!(revisions[0].provider_epoch, epoch(9));
        assert_eq!(revisions[0].document, fallback.document);
        assert!(revisions
            .iter()
            .all(|revision| revision.body_identity.is_none()));
        assert!(revisions
            .iter()
            .all(|revision| revision.facets.body.is_none()));
        assert!(revisions
            .iter()
            .any(|revision| revision.symbol.as_deref() == Some("outer::inner")));

        let LanguageResponse::EntityRevisions {
            revisions: repeated,
        } = invoke(
            &mut kernel,
            LanguageCommand::IngestDocumentSymbols {
                observation_id: "lsp-symbols-1".into(),
                repository_id: "repo-1".into(),
            },
        )
        .unwrap()
        else {
            panic!("expected idempotent entity revisions");
        };
        assert_eq!(repeated, revisions);

        let LanguageResponse::EntityRevisions { revisions: located } = invoke(
            &mut kernel,
            LanguageCommand::IngestDocumentSymbolsWithEncoding {
                observation_id: "lsp-symbols-1".into(),
                repository_id: "repo-1".into(),
                position_encoding: phenix_sdk::CodePositionEncoding::Utf16,
            },
        )
        .unwrap() else {
            panic!("expected source-located entity revisions");
        };
        assert_eq!(located, revisions);

        let first = &revisions[0];
        let LanguageResponse::EntitySourceLocator {
            locator: Some(locator),
        } = invoke(
            &mut kernel,
            LanguageCommand::GetEntitySourceLocator {
                repository_id: first.entity.repository_id.clone(),
                entity_id: first.entity.id.clone(),
                revision: first.revision.clone(),
            },
        )
        .unwrap()
        else {
            panic!("expected source locator for normalized entity revision");
        };
        assert_eq!(locator.entity, first.entity);
        assert_eq!(locator.revision, first.revision);
        assert_eq!(locator.document, fallback.document);
        assert_eq!(
            locator.position_encoding,
            phenix_sdk::CodePositionEncoding::Utf16
        );
        assert_eq!(locator.range.start.line, 0);
        assert_eq!(locator.range.start.character, 0);
        assert_eq!(locator.range.end.line, 0);
        assert_eq!(locator.range.end.character, 34);
        assert_eq!(locator.selection_range.start.character, 7);
        assert_eq!(locator.selection_range.end.character, 12);
        assert!(locator.body_range.is_none());

        let body_error = invoke(
            &mut kernel,
            LanguageCommand::ReadEntityBody {
                repository_id: first.entity.repository_id.clone(),
                entity_id: first.entity.id.clone(),
                revision: first.revision.clone(),
                max_bytes: 1024,
            },
        )
        .unwrap_err();
        assert!(body_error.contains("no exact semantic body range"));

        let semantic_edit_error = invoke(
            &mut kernel,
            LanguageCommand::ReplaceEntityBody {
                operation_id: "unsupported-body-edit".into(),
                repository_id: first.entity.repository_id.clone(),
                entity_id: first.entity.id.clone(),
                revision: first.revision.clone(),
                content: "{ 42 }".into(),
            },
        )
        .unwrap_err();
        assert!(semantic_edit_error.contains("no exact semantic body range"));
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn outer() { fn inner() {} }\n"
        );

        let expected_version = WorkspaceFileVersion::Present {
            content_hash: fallback
                .document
                .file_version
                .as_deref()
                .and_then(|revision| revision.strip_prefix("sha256:"))
                .expect("workspace fallback has a canonical source revision")
                .to_owned(),
        };
        assert!(matches!(
            invoke_workspace(
                &mut kernel,
                WorkspaceCommand::Write {
                    path: "src/lib.rs".into(),
                    content: "pub fn textual_fallback() {}\n".into(),
                    expected_version,
                },
            )
            .unwrap(),
            WorkspaceResponse::Written { .. }
        ));
        assert_eq!(
            fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "pub fn textual_fallback() {}\n"
        );

        let LanguageResponse::EntityChanges { page } = invoke(
            &mut kernel,
            LanguageCommand::GetEntityChanges {
                repository_id: "repo-1".into(),
                after_sequence: 0,
                limit: 10,
            },
        )
        .unwrap() else {
            panic!("expected entity change page");
        };
        assert_eq!(page.events.len(), 2);

        fs::write(
            root.join("src/lib.rs"),
            "pub fn changed_after_observation() {}\n",
        )
        .unwrap();
        let stale = invoke(
            &mut kernel,
            LanguageCommand::IngestDocumentSymbols {
                observation_id: "lsp-symbols-1".into(),
                repository_id: "repo-1".into(),
            },
        )
        .unwrap_err();
        assert!(stale.contains("source revision is stale"));

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn logical_entity_revision_map_survives_restart_and_provider_change() {
        let path = temp_db("entity-revision-map");
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let revision = CodeEntityRevision {
            entity: entity.clone(),
            revision: "revision-1".into(),
            sequence: 1,
            document: LanguageDocumentIdentity {
                path: "src/old.rs".into(),
                file_version: Some("sha256:old".into()),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some("crate::old_name".into()),
            name: "old_name".into(),
            signature_identity: Some("signature-1".into()),
            body_identity: Some("body-1".into()),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: CodeEntityFacetRevisions {
                existence: "existence-1".into(),
                name_location: "location-1".into(),
                signature: Some("signature-facet-1".into()),
                body: Some("body-facet-1".into()),
                relations: BTreeMap::new(),
            },
        };

        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: revision.clone(),
                },
            )
            .unwrap();
        }

        let mut kernel = kernel_with(&path);
        let loaded = invoke(
            &mut kernel,
            LanguageCommand::GetEntityRevision {
                repository_id: entity.repository_id.clone(),
                entity_id: entity.id.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            loaded,
            LanguageResponse::EntityRevision {
                revision: Some(revision.clone()),
            }
        );

        let mut renamed = revision;
        renamed.revision = "revision-2".into();
        renamed.sequence = 2;
        renamed.document.path = "src/new.rs".into();
        renamed.name = "new_name".into();
        renamed.provider_id = "scip".into();
        renamed.provider_epoch = epoch(2);
        renamed.facets.name_location = "location-2".into();
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: renamed.clone(),
            },
        )
        .unwrap();

        let loaded = invoke(
            &mut kernel,
            LanguageCommand::GetEntityRevision {
                repository_id: entity.repository_id,
                entity_id: entity.id,
            },
        )
        .unwrap();
        assert_eq!(
            loaded,
            LanguageResponse::EntityRevision {
                revision: Some(renamed),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn identity_rebuild_requires_catch_up_through_latest_change_sequence() {
        let path = temp_db("identity-rebuild-catch-up");
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let revision = |id: &str, sequence: u64, location: &str| CodeEntityRevision {
            entity: entity.clone(),
            revision: id.into(),
            sequence,
            document: LanguageDocumentIdentity {
                path: location.into(),
                file_version: Some(format!("sha256:{id}")),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some("crate::run".into()),
            name: "run".into(),
            signature_identity: Some("signature".into()),
            body_identity: Some(format!("body-{id}")),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: CodeEntityFacetRevisions {
                existence: "existence".into(),
                name_location: format!("location-{location}"),
                signature: Some("signature".into()),
                body: Some(format!("body-{id}")),
                relations: BTreeMap::new(),
            },
        };

        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: revision("revision-1", 1, "src/lib.rs"),
                },
            )
            .unwrap();

            assert_eq!(
                invoke(
                    &mut kernel,
                    LanguageCommand::BeginIdentityRebuild {
                        repository_id: "repo-1".into(),
                    },
                )
                .unwrap(),
                LanguageResponse::IdentityRebuild {
                    checkpoint: CodeIdentityRebuildCheckpoint {
                        repository_id: "repo-1".into(),
                        required_through_sequence: 1,
                    },
                }
            );

            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: revision("revision-2", 2, "src/moved.rs"),
                },
            )
            .unwrap();

            let error = invoke(
                &mut kernel,
                LanguageCommand::CompleteIdentityRebuild {
                    repository_id: "repo-1".into(),
                    applied_through_sequence: 1,
                },
            )
            .unwrap_err();
            assert!(error.contains("not caught up"));
            assert!(error.contains("current sequence is 2"));

            let LanguageResponse::EntityChanges { page } = invoke(
                &mut kernel,
                LanguageCommand::GetEntityChanges {
                    repository_id: "repo-1".into(),
                    after_sequence: 1,
                    limit: 100,
                },
            )
            .unwrap() else {
                panic!("expected entity change page");
            };
            assert_eq!(page.events.len(), 1);
            assert_eq!(page.events[0].sequence, 2);
            assert!(page.caught_up);

            assert_eq!(
                invoke(
                    &mut kernel,
                    LanguageCommand::CompleteIdentityRebuild {
                        repository_id: "repo-1".into(),
                        applied_through_sequence: 2,
                    },
                )
                .unwrap(),
                LanguageResponse::IdentityRebuild {
                    checkpoint: CodeIdentityRebuildCheckpoint {
                        repository_id: "repo-1".into(),
                        required_through_sequence: 2,
                    },
                }
            );
        }

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke(
                &mut restored,
                LanguageCommand::GetIdentityContinuity {
                    repository_id: "repo-1".into(),
                },
            )
            .unwrap(),
            LanguageResponse::IdentityContinuity {
                state: Some(CodeIdentityContinuityState {
                    repository_id: "repo-1".into(),
                    status: CodeIdentityContinuityStatus::Available,
                    reason: None,
                }),
            }
        );

        let error = invoke(
            &mut restored,
            LanguageCommand::SetIdentityContinuity {
                state: CodeIdentityContinuityState {
                    repository_id: "repo-1".into(),
                    status: CodeIdentityContinuityStatus::Rebuilding,
                    reason: None,
                },
            },
        )
        .unwrap_err();
        assert!(error.contains("BeginIdentityRebuild"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn unavailable_identity_continuity_survives_restart() {
        let path = temp_db("identity-continuity");
        let state = CodeIdentityContinuityState {
            repository_id: "repo-1".into(),
            status: CodeIdentityContinuityStatus::Unavailable,
            reason: Some("durable identity map was lost".into()),
        };
        {
            let mut kernel = kernel_with(&path);
            assert_eq!(
                invoke(
                    &mut kernel,
                    LanguageCommand::SetIdentityContinuity {
                        state: state.clone(),
                    },
                )
                .unwrap(),
                LanguageResponse::IdentityContinuity {
                    state: Some(state.clone()),
                }
            );
        }

        let mut kernel = kernel_with(&path);
        assert_eq!(
            invoke(
                &mut kernel,
                LanguageCommand::GetIdentityContinuity {
                    repository_id: "repo-1".into(),
                },
            )
            .unwrap(),
            LanguageResponse::IdentityContinuity { state: Some(state) }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn entity_facets_are_queryable_as_typed_current_references() {
        let path = temp_db("entity-facet-query");
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let revision = CodeEntityRevision {
            entity: entity.clone(),
            revision: "revision-1".into(),
            sequence: 1,
            document: LanguageDocumentIdentity {
                path: "src/lib.rs".into(),
                file_version: Some("sha256:file".into()),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some("crate::run".into()),
            name: "run".into(),
            signature_identity: Some("signature-1".into()),
            body_identity: Some("body-1".into()),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: CodeEntityFacetRevisions {
                existence: "existence-1".into(),
                name_location: "location-1".into(),
                signature: Some("signature-1".into()),
                body: Some("body-1".into()),
                relations: BTreeMap::from([("callers".into(), "callers-1".into())]),
            },
        };
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: revision.clone(),
            },
        )
        .unwrap();

        assert_eq!(
            invoke(
                &mut kernel,
                LanguageCommand::GetEntityFacet {
                    repository_id: entity.repository_id.clone(),
                    entity_id: entity.id.clone(),
                    facet: CodeEntityFacet::Body,
                },
            )
            .unwrap(),
            LanguageResponse::EntityFacet {
                reference: Some(revision.facet_reference(CodeEntityFacet::Body).unwrap()),
            }
        );
        assert_eq!(
            invoke(
                &mut kernel,
                LanguageCommand::GetEntityFacet {
                    repository_id: entity.repository_id,
                    entity_id: entity.id,
                    facet: CodeEntityFacet::Relation {
                        name: "implementations".into(),
                    },
                },
            )
            .unwrap(),
            LanguageResponse::EntityFacet { reference: None }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn facet_change_query_only_reports_changed_neighborhoods() {
        let path = temp_db("entity-facet-changes");
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let make =
            |revision: &str, sequence: u64, location: &str, callers: &str| CodeEntityRevision {
                entity: entity.clone(),
                revision: revision.into(),
                sequence,
                document: LanguageDocumentIdentity {
                    path: location.into(),
                    file_version: Some(format!("sha256:{revision}")),
                    provenance: DocumentProvenance::WorkspaceBacked,
                },
                symbol: Some("crate::run".into()),
                name: "run".into(),
                signature_identity: Some("signature-stable".into()),
                body_identity: Some("body-stable".into()),
                provider_id: "rust-analyzer".into(),
                provider_epoch: epoch(1),
                facets: CodeEntityFacetRevisions {
                    existence: "existence-stable".into(),
                    name_location: format!("location-{sequence}"),
                    signature: Some("signature-stable".into()),
                    body: Some("body-stable".into()),
                    relations: BTreeMap::from([("callers".into(), callers.into())]),
                },
            };
        let first = make("revision-1", 1, "src/old.rs", "callers-1");
        let second = make("revision-2", 2, "src/new.rs", "callers-2");
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: first.clone(),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision { revision: second },
        )
        .unwrap();

        assert_eq!(
            invoke(
                &mut kernel,
                LanguageCommand::GetEntityFacetChanges {
                    repository_id: entity.repository_id,
                    entity_id: entity.id,
                    from_revision: first.revision,
                },
            )
            .unwrap(),
            LanguageResponse::EntityFacetChanges {
                changes: Some(CodeEntityFacetChanges {
                    existence: false,
                    name_location: true,
                    signature: false,
                    body: false,
                    relations: vec!["callers".into()],
                }),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn entity_change_stream_is_repository_global_paginated_and_restart_safe() {
        let path = temp_db("entity-change-stream");
        let revision = |entity_id: &str,
                        revision_id: &str,
                        entity_sequence: u64,
                        location: &str,
                        callers: &str| CodeEntityRevision {
            entity: LogicalCodeEntity {
                id: entity_id.into(),
                repository_id: "repo-1".into(),
            },
            revision: revision_id.into(),
            sequence: entity_sequence,
            document: LanguageDocumentIdentity {
                path: location.into(),
                file_version: Some(format!("sha256:{revision_id}")),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some(format!("crate::{entity_id}")),
            name: entity_id.into(),
            signature_identity: Some(format!("signature-{entity_id}")),
            body_identity: Some(format!("body-{entity_id}")),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: CodeEntityFacetRevisions {
                existence: format!("existence-{entity_id}"),
                name_location: format!("location-{location}"),
                signature: Some(format!("signature-{entity_id}")),
                body: Some(format!("body-{entity_id}")),
                relations: BTreeMap::from([("callers".into(), callers.into())]),
            },
        };

        let a1 = revision("a", "a-1", 1, "src/a.rs", "callers-a-1");
        let b1 = revision("b", "b-1", 1, "src/b.rs", "callers-b-1");
        let a2 = revision("a", "a-2", 2, "src/moved/a.rs", "callers-a-2");

        {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: a1.clone(),
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: b1.clone(),
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: a2.clone(),
                },
            )
            .unwrap();

            let first = invoke(
                &mut kernel,
                LanguageCommand::GetEntityChanges {
                    repository_id: "repo-1".into(),
                    after_sequence: 0,
                    limit: 2,
                },
            )
            .unwrap();
            let LanguageResponse::EntityChanges { page } = first else {
                panic!("expected entity change page");
            };
            assert_eq!(page.current_sequence, 3);
            assert_eq!(page.next_after_sequence, 2);
            assert!(!page.caught_up);
            assert_eq!(page.events.len(), 2);
            assert_eq!(page.events[0].sequence, 1);
            assert_eq!(page.events[0].entity.id, "a");
            assert_eq!(page.events[1].sequence, 2);
            assert_eq!(page.events[1].entity.id, "b");
        }

        {
            let mut kernel = kernel_with(&path);
            let tail = invoke(
                &mut kernel,
                LanguageCommand::GetEntityChanges {
                    repository_id: "repo-1".into(),
                    after_sequence: 2,
                    limit: 100,
                },
            )
            .unwrap();
            let LanguageResponse::EntityChanges { page } = tail else {
                panic!("expected entity change page");
            };
            assert!(page.caught_up);
            assert_eq!(page.current_sequence, 3);
            assert_eq!(page.next_after_sequence, 3);
            assert_eq!(page.events.len(), 1);
            let event = &page.events[0];
            assert_eq!(event.sequence, 3);
            assert_eq!(event.entity.id, "a");
            assert_eq!(event.previous_revision.as_deref(), Some("a-1"));
            assert_eq!(event.revision, "a-2");
            assert!(event.changes.name_location);
            assert!(!event.changes.body);
            assert_eq!(event.changes.relations, vec!["callers".to_owned()]);

            // Replaying immutable historical evidence must not create a duplicate stream event
            // or move the current entity pointer backwards.
            invoke(
                &mut kernel,
                LanguageCommand::RecordEntityRevision {
                    revision: a1.clone(),
                },
            )
            .unwrap();
            let after_replay = invoke(
                &mut kernel,
                LanguageCommand::GetEntityChanges {
                    repository_id: "repo-1".into(),
                    after_sequence: 3,
                    limit: 100,
                },
            )
            .unwrap();
            assert_eq!(
                after_replay,
                LanguageResponse::EntityChanges {
                    page: CodeEntityChangePage {
                        repository_id: "repo-1".into(),
                        after_sequence: 3,
                        current_sequence: 3,
                        events: Vec::new(),
                        next_after_sequence: 3,
                        caught_up: true,
                    },
                }
            );
        }

        let _ = fs::remove_file(path);
    }

    #[test]
    fn out_of_order_new_revision_cannot_replace_current_entity_revision() {
        let path = temp_db("entity-sequence");
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let make = |id: &str, sequence: u64| CodeEntityRevision {
            entity: entity.clone(),
            revision: id.into(),
            sequence,
            document: LanguageDocumentIdentity {
                path: "src/lib.rs".into(),
                file_version: Some(format!("sha256:{id}")),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some("crate::run".into()),
            name: "run".into(),
            signature_identity: Some(format!("signature-{id}")),
            body_identity: Some(format!("body-{id}")),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: CodeEntityFacetRevisions {
                existence: format!("existence-{id}"),
                name_location: format!("location-{id}"),
                signature: Some(format!("signature-{id}")),
                body: Some(format!("body-{id}")),
                relations: BTreeMap::new(),
            },
        };
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: make("revision-2", 2),
            },
        )
        .unwrap();

        let error = invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: make("revision-1-late", 1),
            },
        )
        .unwrap_err();
        assert!(error.contains("must advance beyond current sequence 2"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn replaying_historical_revision_does_not_roll_back_current_entity_revision() {
        let path = temp_db("entity-revision-replay");
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let revision = |name: &str, id: &str| CodeEntityRevision {
            entity: entity.clone(),
            revision: id.into(),
            sequence: if id == "revision-1" { 1 } else { 2 },
            document: LanguageDocumentIdentity {
                path: format!("src/{name}.rs"),
                file_version: Some(format!("sha256:{id}")),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some(format!("crate::{name}")),
            name: name.into(),
            signature_identity: Some(format!("signature-{id}")),
            body_identity: Some(format!("body-{id}")),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: CodeEntityFacetRevisions {
                existence: format!("existence-{id}"),
                name_location: format!("location-{id}"),
                signature: Some(format!("signature-facet-{id}")),
                body: Some(format!("body-facet-{id}")),
                relations: BTreeMap::new(),
            },
        };
        let first = revision("old_name", "revision-1");
        let second = revision("new_name", "revision-2");
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: first.clone(),
            },
        )
        .unwrap();
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision {
                revision: second.clone(),
            },
        )
        .unwrap();

        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityRevision { revision: first },
        )
        .unwrap();

        assert_eq!(
            invoke(
                &mut kernel,
                LanguageCommand::GetEntityRevision {
                    repository_id: entity.repository_id,
                    entity_id: entity.id,
                },
            )
            .unwrap(),
            LanguageResponse::EntityRevision {
                revision: Some(second),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn logical_entity_identity_is_independent_of_revision_location_and_name() {
        let entity = LogicalCodeEntity {
            id: "entity-1".into(),
            repository_id: "repo-1".into(),
        };
        let facets = CodeEntityFacetRevisions {
            existence: "existence-1".into(),
            name_location: "location-1".into(),
            signature: Some("signature-1".into()),
            body: Some("body-1".into()),
            relations: BTreeMap::new(),
        };
        let before = CodeEntityRevision {
            entity: entity.clone(),
            revision: "revision-1".into(),
            sequence: 1,
            document: LanguageDocumentIdentity {
                path: "src/old.rs".into(),
                file_version: Some("sha256:old".into()),
                provenance: DocumentProvenance::WorkspaceBacked,
            },
            symbol: Some("crate::old_name".into()),
            name: "old_name".into(),
            signature_identity: Some("signature".into()),
            body_identity: Some("body".into()),
            provider_id: "rust-analyzer".into(),
            provider_epoch: epoch(1),
            facets: facets.clone(),
        };
        let mut after = before.clone();
        after.revision = "revision-2".into();
        after.sequence = 2;
        after.document.path = "src/new.rs".into();
        after.name = "new_name".into();
        after.facets.name_location = "location-2".into();

        assert_eq!(before.entity, after.entity);
        assert_ne!(before.revision, after.revision);
        assert_ne!(before.document.path, after.document.path);
        assert_ne!(before.name, after.name);
    }

    #[test]
    fn confirmed_move_lineage_preserves_identity_and_survives_restart() {
        let path = temp_db("entity-confirmed-move-lineage");
        let lineage = CodeEntityLineage {
            from_entity_id: "entity-1".into(),
            to_entity_id: "entity-1".into(),
            kind: CodeEntityLineageKind::Move,
            confidence: CodeEntityLineageConfidence::Confirmed,
            evidence_observation_ids: vec!["language-move-1".into()],
        };
        {
            let mut kernel = kernel_with(&path);
            assert_eq!(
                invoke(
                    &mut kernel,
                    LanguageCommand::RecordEntityLineage {
                        repository_id: "repo-1".into(),
                        lineage: lineage.clone(),
                    },
                )
                .unwrap(),
                LanguageResponse::EntityLineage {
                    lineage: Some(lineage.clone()),
                }
            );
            let invalid = invoke(
                &mut kernel,
                LanguageCommand::RecordEntityLineage {
                    repository_id: "repo-1".into(),
                    lineage: CodeEntityLineage {
                        to_entity_id: "entity-2".into(),
                        ..lineage.clone()
                    },
                },
            )
            .unwrap_err();
            assert!(invalid.contains("must preserve logical entity identity"));
        }

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke(
                &mut restored,
                LanguageCommand::GetEntityLineage {
                    repository_id: "repo-1".into(),
                    from_entity_id: "entity-1".into(),
                    to_entity_id: "entity-1".into(),
                    kind: CodeEntityLineageKind::Move,
                },
            )
            .unwrap(),
            LanguageResponse::EntityLineage {
                lineage: Some(lineage),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn replacement_lineage_requires_and_preserves_distinct_identity() {
        let path = temp_db("entity-replacement-lineage");
        let lineage = CodeEntityLineage {
            from_entity_id: "entity-old".into(),
            to_entity_id: "entity-new".into(),
            kind: CodeEntityLineageKind::Replacement,
            confidence: CodeEntityLineageConfidence::Confirmed,
            evidence_observation_ids: vec!["language-replacement-1".into()],
        };
        let mut kernel = kernel_with(&path);
        invoke(
            &mut kernel,
            LanguageCommand::RecordEntityLineage {
                repository_id: "repo-1".into(),
                lineage: lineage.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            invoke(
                &mut kernel,
                LanguageCommand::GetEntityLineage {
                    repository_id: "repo-1".into(),
                    from_entity_id: "entity-old".into(),
                    to_entity_id: "entity-new".into(),
                    kind: CodeEntityLineageKind::Replacement,
                },
            )
            .unwrap(),
            LanguageResponse::EntityLineage {
                lineage: Some(lineage.clone()),
            }
        );
        let invalid = invoke(
            &mut kernel,
            LanguageCommand::RecordEntityLineage {
                repository_id: "repo-1".into(),
                lineage: CodeEntityLineage {
                    to_entity_id: "entity-old".into(),
                    ..lineage
                },
            },
        )
        .unwrap_err();
        assert!(invalid.contains("requires a distinct target identity"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn ambiguous_lineage_can_remain_tentative_without_forcing_identity() {
        let lineage = CodeEntityLineage {
            from_entity_id: "entity-1".into(),
            to_entity_id: "entity-2".into(),
            kind: CodeEntityLineageKind::Split,
            confidence: CodeEntityLineageConfidence::Tentative,
            evidence_observation_ids: vec!["language-1".into()],
        };

        assert_ne!(lineage.from_entity_id, lineage.to_entity_id);
        assert_eq!(lineage.confidence, CodeEntityLineageConfidence::Tentative);
    }

    #[test]
    fn zero_provider_epoch_is_rejected_at_decode_boundary() {
        assert!(ProviderEpoch::new(0).is_err());
        assert!(serde_json::from_value::<ProviderEpoch>(serde_json::json!(0)).is_err());
        let value = PhenixValue::U64(0);
        assert!(ProviderEpoch::try_from(Project(&value)).is_err());
    }

    #[test]
    fn consumed_observations_are_durable_but_provider_and_diagnostics_are_not() {
        let path = temp_db("language-observation");
        {
            let mut kernel = kernel_with(&path);
            activate(&mut kernel, 1);
            invoke(
                &mut kernel,
                LanguageCommand::PublishDiagnostics {
                    workspace_id: "workspace".into(),
                    provider_id: "rust-analyzer".into(),
                    epoch: epoch(1),
                    result: diagnostics_result(),
                },
            )
            .unwrap();
            invoke(
                &mut kernel,
                LanguageCommand::Consume {
                    observation_id: "language-1".into(),
                    execution_id: "execution-1".into(),
                    workspace_id: "workspace".into(),
                    provider_id: "rust-analyzer".into(),
                    epoch: epoch(1),
                    result: workspace_result(LanguageOperationKind::Definition),
                },
            )
            .unwrap();
        }

        let mut restored = kernel_with(&path);
        assert!(matches!(
            invoke(
                &mut restored,
                LanguageCommand::GetObservation {
                    observation_id: "language-1".into(),
                }
            )
            .unwrap(),
            LanguageResponse::Observation {
                observation: Some(_)
            }
        ));
        assert_eq!(
            invoke(
                &mut restored,
                LanguageCommand::CurrentDiagnostics {
                    workspace_id: "workspace".into(),
                }
            )
            .unwrap(),
            LanguageResponse::Diagnostics { result: None }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn stale_provider_epoch_cannot_record_a_successful_observation() {
        let path = temp_db("language-provider-change");
        let mut kernel = kernel_with(&path);
        activate(&mut kernel, 1);
        activate(&mut kernel, 2);
        let error = invoke(
            &mut kernel,
            LanguageCommand::Consume {
                observation_id: "language-1".into(),
                execution_id: "execution-1".into(),
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(1),
                result: workspace_result(LanguageOperationKind::Hover),
            },
        )
        .unwrap_err();
        assert!(error.contains("ProviderChanged"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn unsaved_frontend_provenance_is_preserved_and_workspace_evidence_requires_a_version() {
        let path = temp_db("language-provenance");
        let mut kernel = kernel_with(&path);
        activate(&mut kernel, 1);
        let unsaved = LanguageOperationResult {
            operation: LanguageOperationKind::Hover,
            payload: serde_json::json!({"text": "hover"}).into(),
            documents: vec![LanguageDocumentIdentity {
                path: "src/lib.rs".into(),
                file_version: None,
                provenance: DocumentProvenance::FrontendUnsaved,
            }],
        };
        let response = invoke(
            &mut kernel,
            LanguageCommand::Consume {
                observation_id: "language-unsaved".into(),
                execution_id: "execution-1".into(),
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(1),
                result: unsaved,
            },
        )
        .unwrap();
        match response {
            LanguageResponse::Observation {
                observation: Some(observation),
            } => assert_eq!(
                observation.result.documents[0].provenance,
                DocumentProvenance::FrontendUnsaved
            ),
            other => panic!("unexpected response: {other:?}"),
        }

        let invalid = LanguageOperationResult {
            operation: LanguageOperationKind::Definition,
            payload: serde_json::json!({}).into(),
            documents: vec![LanguageDocumentIdentity {
                path: "src/lib.rs".into(),
                file_version: None,
                provenance: DocumentProvenance::WorkspaceBacked,
            }],
        };
        let error = invoke(
            &mut kernel,
            LanguageCommand::Consume {
                observation_id: "language-invalid".into(),
                execution_id: "execution-1".into(),
                workspace_id: "workspace".into(),
                provider_id: "rust-analyzer".into(),
                epoch: epoch(1),
                result: invalid,
            },
        )
        .unwrap_err();
        assert!(error.contains("exact file version"));
        let _ = fs::remove_file(path);
    }
}
