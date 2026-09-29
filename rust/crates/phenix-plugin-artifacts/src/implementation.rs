use phenix_core::{
    ArtifactRevision, Authority, CapabilityId, ContentLocator, ContentReference, PluginContext,
    PluginInstance, PluginManifest, ResourceNamespace, ServiceId, TransactionOp,
};
use phenix_sdk::StaticPluginDefinition;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ARTIFACT_SERVICE: &str = "phenix.artifacts@1";
const ARTIFACT_NAMESPACE: &str = "phenix.artifacts.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

type ArtifactContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ArtifactProvenance {
    pub producer: String,
    pub provider_identity: Option<String>,
    pub configuration_identity: Option<String>,
    pub source_observations: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ArtifactRecord {
    pub id: String,
    pub content_identity: String,
    pub content: Vec<u8>,
    pub provenance: ArtifactProvenance,
}

impl ArtifactRecord {
    #[must_use]
    pub fn content_reference(&self, media_type: impl Into<String>) -> ContentReference {
        ContentReference::new(
            &self.content,
            media_type,
            ContentLocator::Service {
                service: artifact_service().as_str().to_owned(),
                resource: self.id.clone(),
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct NormalizedReadRequest {
    pub resource: String,
    pub parameters: BTreeMap<String, String>,
    pub presentation_hint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ReadProviderIdentity {
    pub provider: String,
    pub contract_version: String,
    pub implementation_identity: String,
    pub configuration_identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct ReadResultRecord {
    pub id: String,
    pub request_identity: String,
    pub provider: ReadProviderIdentity,
    pub artifact_id: String,
    pub content_identity: String,
    pub dependencies: BTreeMap<String, String>,
    pub invocation_provenance: String,
}

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue,
)]
#[serde(rename_all = "snake_case")]
pub enum RevalidationVerdict {
    StillValid,
    Invalid,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
pub struct RevalidationRecord {
    pub result_id: String,
    pub provider: ReadProviderIdentity,
    pub current_dependencies: BTreeMap<String, String>,
    pub verdict: RevalidationVerdict,
    pub provenance: String,
}

impl RevalidationRecord {
    #[must_use]
    pub fn reusable(&self) -> bool {
        self.verdict == RevalidationVerdict::StillValid
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum ArtifactCommand {
    Store {
        content: Vec<u8>,
        provenance: ArtifactProvenance,
    },
    Get {
        id: String,
        content_identity: String,
    },
    RecordRead {
        request: NormalizedReadRequest,
        provider: ReadProviderIdentity,
        invocation_provenance: String,
        content: Vec<u8>,
        dependencies: BTreeMap<String, String>,
    },
    LookupRead {
        request: NormalizedReadRequest,
        provider: ReadProviderIdentity,
        dependencies: BTreeMap<String, String>,
    },
    Revalidate {
        result_id: String,
        provider: ReadProviderIdentity,
        current_dependencies: BTreeMap<String, String>,
        verdict: RevalidationVerdict,
        provenance: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, phenix_sdk_macros::PhenixValue)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum ArtifactResponse {
    Stored {
        artifact: ArtifactRecord,
        reused: bool,
    },
    Artifact {
        artifact: Option<ArtifactRecord>,
    },
    ReadRecorded {
        result: ReadResultRecord,
        artifact: ArtifactRecord,
        reused: bool,
    },
    ReadLookup {
        result: Option<ReadResultRecord>,
    },
    Revalidated {
        record: RevalidationRecord,
    },
}

struct StoredArtifact {
    artifact: ArtifactRecord,
    reused: bool,
}

struct ArtifactStore;

#[phenix_sdk::resource(schema = 1)]
impl ArtifactStore {}

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(export("phenix.artifacts@1"), terminal, priority = 100)]
    fn handle(
        &self,
        context: &phenix_sdk::PluginContext<'_, '_, ()>,
        command: ArtifactCommand,
    ) -> Result<ArtifactResponse, String> {
        handle(context, command)
    }
}

#[phenix_sdk::plugin(id = "phenix.artifacts", authority = persistence_authority())]
pub struct Plugin {
    #[phenix(component, id = "phenix.artifacts")]
    api: Api,

    #[phenix(resource, id = "phenix.artifacts.state")]
    _state: phenix_sdk::Durable<ArtifactStore>,
}

#[must_use]
pub fn artifact_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn artifact_factory() -> Box<dyn PluginInstance> {
    phenix_sdk::StaticPluginComponentDispatch::into_plugin_instance(Plugin {
        api: Api,
        _state: phenix_sdk::Durable::new(),
    })
}

#[must_use]
pub fn artifact_durable_schema_registrations() -> Vec<phenix_core::DurableSchemaRegistration> {
    <Plugin as phenix_sdk::StaticPluginResources>::durable_schema_registrations(
        &artifact_manifest().id,
    )
}

#[must_use]
pub fn artifact_service() -> ServiceId {
    ServiceId::parse(ARTIFACT_SERVICE).expect("static service id is valid")
}

fn artifact_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(ARTIFACT_NAMESPACE).expect("static namespace is valid")
}

fn persistence_authority() -> Authority {
    Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ])
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::parse(value).expect("static capability is valid")
}

fn handle(
    context: &ArtifactContext<'_, '_>,
    command: ArtifactCommand,
) -> Result<ArtifactResponse, String> {
    match command {
        ArtifactCommand::Store {
            content,
            provenance,
        } => store(context, content, provenance),
        ArtifactCommand::Get {
            id,
            content_identity,
        } => Ok(ArtifactResponse::Artifact {
            artifact: read_exact(context, &id, &content_identity)?,
        }),
        ArtifactCommand::RecordRead {
            request,
            provider,
            invocation_provenance,
            content,
            dependencies,
        } => record_read(
            context,
            request,
            provider,
            invocation_provenance,
            content,
            dependencies,
        ),
        ArtifactCommand::LookupRead {
            request,
            provider,
            dependencies,
        } => Ok(ArtifactResponse::ReadLookup {
            result: lookup_read(context, &request, &provider, &dependencies)?,
        }),
        ArtifactCommand::Revalidate {
            result_id,
            provider,
            current_dependencies,
            verdict,
            provenance,
        } => revalidate(
            context,
            result_id,
            provider,
            current_dependencies,
            verdict,
            provenance,
        ),
    }
}

fn store(
    context: &ArtifactContext<'_, '_>,
    content: Vec<u8>,
    provenance: ArtifactProvenance,
) -> Result<ArtifactResponse, String> {
    let StoredArtifact { artifact, reused } = store_artifact(context, content, provenance)?;
    Ok(ArtifactResponse::Stored { artifact, reused })
}

fn store_artifact(
    context: &ArtifactContext<'_, '_>,
    content: Vec<u8>,
    provenance: ArtifactProvenance,
) -> Result<StoredArtifact, String> {
    let content_identity = exact_content_identity(&content);
    let id = format!("artifact:{content_identity}");
    if let Some(existing) = read_record(context, &id)? {
        if existing.content_identity != content_identity || existing.content != content {
            return Err(format!("artifact identity collision: {id}"));
        }
        return Ok(StoredArtifact {
            artifact: existing,
            reused: true,
        });
    }

    let artifact = ArtifactRecord {
        id,
        content_identity,
        content,
        provenance,
    };
    context
        .kernel
        .transact_durable(
            &artifact_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: artifact_key(&artifact.id),
                    expected: None,
                },
                TransactionOp::Put {
                    key: artifact_key(&artifact.id),
                    value: serde_json::to_vec(&artifact).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(StoredArtifact {
        artifact,
        reused: false,
    })
}

fn read_record(
    context: &ArtifactContext<'_, '_>,
    id: &str,
) -> Result<Option<ArtifactRecord>, String> {
    context
        .kernel
        .read_durable(&artifact_namespace(), &artifact_key(id))
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_exact(
    context: &ArtifactContext<'_, '_>,
    id: &str,
    content_identity: &str,
) -> Result<Option<ArtifactRecord>, String> {
    Ok(read_record(context, id)?.filter(|artifact| artifact.content_identity == content_identity))
}

fn record_read(
    context: &ArtifactContext<'_, '_>,
    request: NormalizedReadRequest,
    provider: ReadProviderIdentity,
    invocation_provenance: String,
    content: Vec<u8>,
    dependencies: BTreeMap<String, String>,
) -> Result<ArtifactResponse, String> {
    if let Some(result) = lookup_read(context, &request, &provider, &dependencies)? {
        let artifact = read_exact(context, &result.artifact_id, &result.content_identity)?
            .ok_or_else(|| format!("missing artifact for reusable read result: {}", result.id))?;
        return Ok(ArtifactResponse::ReadRecorded {
            result,
            artifact,
            reused: true,
        });
    }

    let artifact = store_artifact(
        context,
        content,
        ArtifactProvenance {
            producer: invocation_provenance.clone(),
            provider_identity: Some(provider.provider.clone()),
            configuration_identity: Some(provider.configuration_identity.clone()),
            source_observations: dependencies.clone(),
        },
    )?
    .artifact;
    let request_identity = normalized_request_identity(&request)?;
    let result = ReadResultRecord {
        id: read_result_identity(
            &request_identity,
            &provider,
            &artifact.content_identity,
            &dependencies,
        )?,
        request_identity: request_identity.clone(),
        provider: provider.clone(),
        artifact_id: artifact.id.clone(),
        content_identity: artifact.content_identity.clone(),
        dependencies,
        invocation_provenance,
    };
    let result_key = read_result_key(&result.id);
    let existing = context
        .kernel
        .read_durable(&artifact_namespace(), &result_key)
        .map_err(|error| error.to_string())?;
    if let Some(existing) = existing {
        let existing: ReadResultRecord =
            serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
        if existing != result {
            return Err(format!("read result identity collision: {}", result.id));
        }
    } else {
        context
            .kernel
            .transact_durable(
                &artifact_namespace(),
                &[
                    TransactionOp::AssertValue {
                        key: result_key.clone(),
                        expected: None,
                    },
                    TransactionOp::Put {
                        key: result_key,
                        value: serde_json::to_vec(&result).map_err(|error| error.to_string())?,
                    },
                    TransactionOp::Put {
                        key: read_index_key(&request_identity, &provider)?,
                        value: serde_json::to_vec(&result.id).map_err(|error| error.to_string())?,
                    },
                ],
            )
            .map_err(|error| error.to_string())?;
    }

    Ok(ArtifactResponse::ReadRecorded {
        result,
        artifact,
        reused: false,
    })
}

fn lookup_read(
    context: &ArtifactContext<'_, '_>,
    request: &NormalizedReadRequest,
    provider: &ReadProviderIdentity,
    dependencies: &BTreeMap<String, String>,
) -> Result<Option<ReadResultRecord>, String> {
    let request_identity = normalized_request_identity(request)?;
    let Some(result_id) = context
        .kernel
        .read_durable(
            &artifact_namespace(),
            &read_index_key(&request_identity, provider)?,
        )
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let result_id: String =
        serde_json::from_slice(&result_id).map_err(|error| error.to_string())?;
    let Some(result) = context
        .kernel
        .read_durable(&artifact_namespace(), &read_result_key(&result_id))
        .map_err(|error| error.to_string())?
    else {
        return Err(format!("missing indexed read result: {result_id}"));
    };
    let result: ReadResultRecord =
        serde_json::from_slice(&result).map_err(|error| error.to_string())?;
    if result.request_identity != request_identity
        || &result.provider != provider
        || &result.dependencies != dependencies
        || read_exact(context, &result.artifact_id, &result.content_identity)?.is_none()
    {
        return Ok(None);
    }
    Ok(Some(result))
}

fn revalidate(
    context: &ArtifactContext<'_, '_>,
    result_id: String,
    provider: ReadProviderIdentity,
    current_dependencies: BTreeMap<String, String>,
    verdict: RevalidationVerdict,
    provenance: String,
) -> Result<ArtifactResponse, String> {
    let result = context
        .kernel
        .read_durable(&artifact_namespace(), &read_result_key(&result_id))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("unknown read result: {result_id}"))?;
    let result: ReadResultRecord =
        serde_json::from_slice(&result).map_err(|error| error.to_string())?;
    if read_exact(context, &result.artifact_id, &result.content_identity)?.is_none() {
        return Err(format!("missing artifact for read result: {result_id}"));
    }
    let record = RevalidationRecord {
        result_id,
        provider,
        current_dependencies,
        verdict,
        provenance,
    };
    context
        .kernel
        .transact_durable(
            &artifact_namespace(),
            &[TransactionOp::Put {
                key: revalidation_key(&record)?,
                value: serde_json::to_vec(&record).map_err(|error| error.to_string())?,
            }],
        )
        .map_err(|error| error.to_string())?;
    Ok(ArtifactResponse::Revalidated { record })
}

fn normalized_request_identity(request: &NormalizedReadRequest) -> Result<String, String> {
    if request.resource.trim().is_empty() {
        return Err("read resource must not be empty".into());
    }
    serde_json::to_string(&(&request.resource, &request.parameters))
        .map(|identity| format!("request:{identity}"))
        .map_err(|error| error.to_string())
}

fn read_result_identity(
    request_identity: &str,
    provider: &ReadProviderIdentity,
    content_identity: &str,
    dependencies: &BTreeMap<String, String>,
) -> Result<String, String> {
    serde_json::to_string(&(request_identity, provider, content_identity, dependencies))
        .map(|identity| format!("read:{identity}"))
        .map_err(|error| error.to_string())
}

fn read_index_key(
    request_identity: &str,
    provider: &ReadProviderIdentity,
) -> Result<String, String> {
    serde_json::to_string(&(request_identity, provider))
        .map(|identity| format!("read-index/{identity}"))
        .map_err(|error| error.to_string())
}

fn revalidation_key(record: &RevalidationRecord) -> Result<String, String> {
    serde_json::to_string(record)
        .map(|identity| format!("revalidation/{identity}"))
        .map_err(|error| error.to_string())
}

fn exact_content_identity(content: &[u8]) -> String {
    ArtifactRevision::from_content(content).to_string()
}

fn artifact_key(id: &str) -> String {
    format!("artifact/{id}")
}

fn read_result_key(id: &str) -> String {
    format!("read-result/{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_component_manifest;
    use phenix_core::{Kernel, LocalPersistence, ResolvedHarness, ResolvedHarnessActivation};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn authority() -> Authority {
        artifact_manifest().maximum_authority
    }

    fn resolved() -> ResolvedHarness {
        ResolvedHarness::resolve_with_durable_schemas(
            [artifact_manifest()],
            [artifact_component_manifest()],
            artifact_durable_schema_registrations(),
            [],
            &authority(),
        )
        .unwrap()
    }

    fn kernel() -> Kernel {
        let resolved = resolved();
        let plugin = artifact_manifest().id;
        let mut kernel = Kernel::new(resolved.kernel_config().clone());
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(plugin, artifact_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn kernel_with(path: &PathBuf) -> Kernel {
        let resolved = resolved();
        let plugin = artifact_manifest().id;
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel = Kernel::with_persistence(resolved.kernel_config().clone(), persistence);
        kernel.activate_resolved_harness(&resolved).unwrap();
        kernel
            .register_embedded_factory(plugin, artifact_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

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

    fn invoke(kernel: &mut Kernel, command: ArtifactCommand) -> ArtifactResponse {
        let input = serde_json::to_vec(&phenix_core::PhenixValue::from(&command)).unwrap();
        let component = artifact_component_manifest();
        let output = kernel
            .invoke_component(
                &component.id,
                &artifact_service(),
                &input,
                &authority(),
                &component.owner,
            )
            .unwrap();
        let output: phenix_core::PhenixValue = serde_json::from_slice(&output).unwrap();
        output.project().unwrap()
    }

    #[test]
    fn artifact_identity_is_bounded_independent_of_payload_size() {
        let small = exact_content_identity(b"small");
        let large = exact_content_identity(&vec![b'x'; 2 * 1024 * 1024]);

        assert_eq!(small.len(), "sha256:".len() + 64);
        assert_eq!(large.len(), "sha256:".len() + 64);
        assert_ne!(small, large);
    }

    #[test]
    fn stored_artifact_id_stays_bounded_for_large_payloads() {
        let mut kernel = kernel();
        let payload = vec![b'x'; 2 * 1024 * 1024];
        let stored = invoke(
            &mut kernel,
            ArtifactCommand::Store {
                content: payload.clone(),
                provenance: provenance(),
            },
        );
        let ArtifactResponse::Stored { artifact, .. } = stored else {
            panic!("unexpected artifact response")
        };

        assert_eq!(artifact.content_identity.len(), "sha256:".len() + 64);
        assert_eq!(artifact.id.len(), "artifact:".len() + "sha256:".len() + 64);
        assert_eq!(
            artifact.content_identity,
            ArtifactRevision::from_content(&payload).to_string()
        );
    }

    #[test]
    fn artifact_projects_to_shared_content_reference() {
        let artifact = ArtifactRecord {
            id: "artifact:fixture".into(),
            content_identity: "legacy-content-identity".into(),
            content: b"fixture-body".to_vec(),
            provenance: provenance(),
        };
        let reference = artifact.content_reference("text/plain");

        assert_eq!(reference.bytes, artifact.content.len());
        assert_eq!(
            reference.digest,
            phenix_core::ArtifactRevision::from_content(&artifact.content)
        );
        assert_eq!(reference.media_type, "text/plain");
        assert_eq!(
            reference.locator,
            ContentLocator::Service {
                service: ARTIFACT_SERVICE.into(),
                resource: artifact.id,
            }
        );
    }

    fn provenance() -> ArtifactProvenance {
        ArtifactProvenance {
            producer: "fixture".into(),
            provider_identity: Some("reader".into()),
            configuration_identity: Some("config-1".into()),
            source_observations: BTreeMap::new(),
        }
    }

    fn request(presentation_hint: &str, mode: &str) -> NormalizedReadRequest {
        NormalizedReadRequest {
            resource: "src/lib.rs".into(),
            parameters: BTreeMap::from([("mode".into(), mode.into())]),
            presentation_hint: Some(presentation_hint.into()),
        }
    }

    fn provider() -> ReadProviderIdentity {
        ReadProviderIdentity {
            provider: "phenix.reader".into(),
            contract_version: "1".into(),
            implementation_identity: "reader-v1".into(),
            configuration_identity: "config-1".into(),
        }
    }

    fn dependencies(version: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("file:src/lib.rs".into(), version.into())])
    }

    #[test]
    fn content_addressed_artifact_is_immutable_and_survives_restart() {
        let path = temp_db("artifacts");
        let stored = {
            let mut kernel = kernel_with(&path);
            invoke(
                &mut kernel,
                ArtifactCommand::Store {
                    content: b"content".to_vec(),
                    provenance: provenance(),
                },
            )
        };
        let ArtifactResponse::Stored {
            artifact,
            reused: false,
        } = stored
        else {
            panic!("unexpected artifact response")
        };

        let mut restored = kernel_with(&path);
        assert_eq!(
            invoke(
                &mut restored,
                ArtifactCommand::Get {
                    id: artifact.id.clone(),
                    content_identity: artifact.content_identity.clone(),
                },
            ),
            ArtifactResponse::Artifact {
                artifact: Some(artifact),
            }
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn equivalent_read_reuses_result_and_ignores_presentation_wording() {
        let mut kernel = kernel();
        let first = invoke(
            &mut kernel,
            ArtifactCommand::RecordRead {
                request: request("please inspect this", "text"),
                provider: provider(),
                invocation_provenance: "invocation-1".into(),
                content: b"first".to_vec(),
                dependencies: dependencies("v1"),
            },
        );
        let ArtifactResponse::ReadRecorded {
            result: first_result,
            artifact: first_artifact,
            reused: false,
        } = first
        else {
            panic!("unexpected first read response")
        };

        let second = invoke(
            &mut kernel,
            ArtifactCommand::RecordRead {
                request: request("different prompt wording", "text"),
                provider: provider(),
                invocation_provenance: "invocation-2".into(),
                content: b"should not replace prior content".to_vec(),
                dependencies: dependencies("v1"),
            },
        );
        assert!(matches!(
            second,
            ArtifactResponse::ReadRecorded {
                result,
                artifact,
                reused: true,
            } if result.id == first_result.id && artifact.id == first_artifact.id
        ));
    }

    #[test]
    fn semantic_request_change_or_dependency_change_prevents_conservative_reuse() {
        let mut kernel = kernel();
        invoke(
            &mut kernel,
            ArtifactCommand::RecordRead {
                request: request("prompt", "text"),
                provider: provider(),
                invocation_provenance: "invocation-1".into(),
                content: b"first".to_vec(),
                dependencies: dependencies("v1"),
            },
        );

        assert_eq!(
            invoke(
                &mut kernel,
                ArtifactCommand::LookupRead {
                    request: request("prompt", "structured"),
                    provider: provider(),
                    dependencies: dependencies("v1"),
                },
            ),
            ArtifactResponse::ReadLookup { result: None }
        );
        assert_eq!(
            invoke(
                &mut kernel,
                ArtifactCommand::LookupRead {
                    request: request("prompt", "text"),
                    provider: provider(),
                    dependencies: dependencies("v2"),
                },
            ),
            ArtifactResponse::ReadLookup { result: None }
        );
    }

    #[test]
    fn semantic_revalidation_is_explicit_and_unknown_does_not_reuse() {
        let mut kernel = kernel();
        let recorded = invoke(
            &mut kernel,
            ArtifactCommand::RecordRead {
                request: request("prompt", "text"),
                provider: provider(),
                invocation_provenance: "invocation-1".into(),
                content: b"first".to_vec(),
                dependencies: dependencies("v1"),
            },
        );
        let ArtifactResponse::ReadRecorded { result, .. } = recorded else {
            panic!("unexpected read response")
        };

        let still_valid = invoke(
            &mut kernel,
            ArtifactCommand::Revalidate {
                result_id: result.id.clone(),
                provider: provider(),
                current_dependencies: dependencies("v2"),
                verdict: RevalidationVerdict::StillValid,
                provenance: "semantic-check-1".into(),
            },
        );
        assert!(matches!(
            still_valid,
            ArtifactResponse::Revalidated { record } if record.reusable()
        ));

        let unknown = invoke(
            &mut kernel,
            ArtifactCommand::Revalidate {
                result_id: result.id,
                provider: provider(),
                current_dependencies: dependencies("v3"),
                verdict: RevalidationVerdict::Unknown,
                provenance: "semantic-check-2".into(),
            },
        );
        assert!(matches!(
            unknown,
            ArtifactResponse::Revalidated { record } if !record.reusable()
        ));
    }
}
