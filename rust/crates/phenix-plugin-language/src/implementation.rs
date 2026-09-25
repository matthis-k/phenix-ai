use phenix_core::{
    Authority, CapabilityId, ComponentInterface, DurableSchema, PluginContext, PluginExecution,
    PluginHost, PluginId, PluginInstance, PluginManifest, ResourceNamespace, ServiceContribution,
    ServiceId, TransactionOp,
};
use phenix_sdk::{
    CodeEntityFacet, CodeEntityFacetChanges, CodeEntityLineage, CodeEntityLineageConfidence,
    CodeEntityLineageKind, CodeEntityRevision, CodeIdentityContinuityState, DiagnosticsResult,
    DocumentProvenance, FileRevisionFallback, LanguageCommand, LanguageDocumentIdentity,
    LanguageObservation, LanguageProviderEpoch, LanguageResponse, ProviderEpoch, WorkspaceCommand,
    WorkspaceFileVersion, WorkspaceInterface, WorkspaceResponse, LANGUAGE_SERVICE,
    WORKSPACE_SERVICE,
};
use std::collections::BTreeMap;

const LANGUAGE_PLUGIN: &str = "phenix.language";
const LANGUAGE_NAMESPACE: &str = "phenix.language.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const WORKSPACE_READ: &str = "workspace.read";

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
                        file_version: Some(content_hash),
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
        LanguageCommand::SetIdentityContinuity { state } => {
            validate_identity("code repository id", &state.repository_id)?;
            if let Some(reason) = &state.reason {
                validate_identity("identity continuity reason", reason)?;
            }
            store_identity_continuity(context, &state)?;
            Ok(LanguageResponse::IdentityContinuity { state: Some(state) })
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

fn store_entity_revision(
    context: &LanguageContext<'_, '_, '_>,
    revision: &CodeEntityRevision,
) -> Result<(), String> {
    let history_key = entity_revision_key(
        &revision.entity.repository_id,
        &revision.entity.id,
        &revision.revision,
    );
    let current_key = entity_current_key(&revision.entity.repository_id, &revision.entity.id);
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
        // The history record and current pointer are written atomically below. Replaying an
        // older immutable revision is therefore already satisfied and must not move current
        // backwards after a newer revision has been recorded.
        return Ok(());
    }

    let current = context
        .kernel
        .read_durable(&language_namespace(), &current_key)
        .map_err(|error| error.to_string())?;
    if let Some(current_bytes) = current.as_ref() {
        let current_revision: CodeEntityRevision =
            serde_json::from_slice(current_bytes).map_err(|error| error.to_string())?;
        if revision.sequence <= current_revision.sequence {
            return Err(format!(
                "code entity revision sequence {} must advance beyond current sequence {}",
                revision.sequence, current_revision.sequence
            ));
        }
    }

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
                TransactionOp::Put {
                    key: history_key,
                    value: encoded.clone(),
                },
                TransactionOp::Put {
                    key: current_key,
                    value: encoded,
                },
            ],
        )
        .map_err(|error| error.to_string())
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

fn entity_current_key(repository_id: &str, entity_id: &str) -> String {
    format!("entity/{repository_id}/{entity_id}/current")
}

fn entity_revision_key(repository_id: &str, entity_id: &str, revision: &str) -> String {
    format!("entity/{repository_id}/{entity_id}/revision/{revision}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{Kernel, KernelConfig, LocalPersistence, PhenixValue, Project};
    use phenix_sdk::{
        CodeEntityFacetChanges, CodeEntityFacetRevisions, CodeEntityLineage,
        CodeEntityLineageConfidence, CodeEntityLineageKind, CodeIdentityContinuityState,
        CodeIdentityContinuityStatus, LanguageOperationKind, LanguageOperationResult,
        LogicalCodeEntity,
    };
    use std::{
        fs,
        path::PathBuf,
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

    fn kernel_with_workspace(path: &PathBuf, root: &PathBuf) -> Kernel {
        let language = language_manifest();
        let language_id = language.id.clone();
        let workspace = phenix_plugin_workspace::workspace_manifest();
        let workspace_id = workspace.id.clone();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel = Kernel::with_persistence(
            KernelConfig::new([language, workspace]).unwrap(),
            persistence,
        );
        kernel
            .register_embedded_factory(language_id, language_factory)
            .unwrap();
        let root = root.clone();
        kernel
            .register_embedded_factory(workspace_id, move || {
                phenix_plugin_workspace::workspace_factory_for(root.clone())
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
