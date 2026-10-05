use phenix_core::{
    Authority, PermissionId, PluginInstance, PluginManifest, ResourceNamespace, ServiceId,
    TransactionOp,
};
use phenix_sdk::{
    JOB_SERVICE, JobCommand, JobResponse, RuntimeResourceKind, RuntimeResourceRecord,
    RuntimeResourceState, StaticPluginDefinition,
};
use std::collections::BTreeSet;

const JOB_NAMESPACE: &str = "phenix.jobs.state";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";
const INDEX_KEY: &str = "index/resources";

type JobContext<'host, 'runtime> = phenix_sdk::PluginContext<'host, 'runtime, ()>;

struct JobStore;

#[phenix_sdk::resource(schema = 1)]
impl JobStore {}

#[phenix_sdk::component]
struct Api;

#[phenix_sdk::component]
impl Api {
    #[phenix(export("phenix.jobs@1"), terminal, priority = 100)]
    fn handle(
        &self,
        context: &phenix_sdk::PluginContext<'_, '_, ()>,
        command: JobCommand,
    ) -> Result<JobResponse, String> {
        handle(context, command)
    }
}

#[phenix_sdk::plugin(id = "phenix.jobs", authority = persistence_authority())]
pub struct Plugin {
    #[phenix(component, id = "phenix.jobs")]
    api: Api,

    #[phenix(resource, id = "phenix.jobs.state")]
    _state: phenix_sdk::Durable<JobStore>,
}

#[must_use]
pub fn job_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn job_factory() -> Box<dyn PluginInstance> {
    phenix_sdk::StaticPluginComponentDispatch::into_plugin_instance(Plugin {
        api: Api,
        _state: phenix_sdk::Durable::new(),
    })
}

#[must_use]
pub fn job_durable_schema_registrations() -> Vec<phenix_core::DurableSchemaRegistration> {
    <Plugin as phenix_sdk::StaticPluginResources>::durable_schema_registrations(&job_manifest().id)
}

#[must_use]
pub fn job_service() -> ServiceId {
    ServiceId::parse(JOB_SERVICE).expect("static service id is valid")
}

fn job_namespace() -> ResourceNamespace {
    ResourceNamespace::parse(JOB_NAMESPACE).expect("static namespace is valid")
}

fn persistence_authority() -> Authority {
    Authority::new([
        capability(PERSISTENCE_SCHEMA),
        capability(PERSISTENCE_READ),
        capability(PERSISTENCE_WRITE),
    ])
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).expect("static capability is valid")
}

fn handle(context: &JobContext<'_, '_>, command: JobCommand) -> Result<JobResponse, String> {
    match command {
        JobCommand::Create {
            id,
            kind,
            owner_execution,
            authority,
        } => Ok(JobResponse::Resource {
            resource: Some(create(context, id, kind, owner_execution, authority)?),
        }),
        JobCommand::Promote { id } => Ok(JobResponse::Resource {
            resource: Some(update(context, &id, |resource| {
                if !matches!(resource.kind, RuntimeResourceKind::Job) {
                    return Err("only jobs may be promoted to workspace lifetime".into());
                }
                if !matches!(resource.state, RuntimeResourceState::Running) {
                    return Err("only running jobs may be promoted".into());
                }
                resource.promoted_to_workspace = true;
                Ok(())
            })?),
        }),
        JobCommand::Complete {
            id,
            code,
            mut output_references,
        } => Ok(JobResponse::Resource {
            resource: Some(update(context, &id, |resource| {
                if !matches!(resource.state, RuntimeResourceState::Running) {
                    return Err("only a running resource may complete".into());
                }
                output_references.sort();
                output_references.dedup();
                resource.output_references = output_references;
                resource.state = RuntimeResourceState::Exited { code };
                Ok(())
            })?),
        }),
        JobCommand::ExecutionTerminated { execution_id } => Ok(JobResponse::Affected {
            resources: mutate_matching(context, &execution_id, |resource| {
                if !resource.promoted_to_workspace
                    && matches!(resource.state, RuntimeResourceState::Running)
                {
                    resource.state = RuntimeResourceState::Revoked {
                        reason: "owner execution terminated".into(),
                    };
                    true
                } else {
                    false
                }
            })?,
        }),
        JobCommand::NarrowAuthority {
            execution_id,
            authority,
        } => Ok(JobResponse::Affected {
            resources: mutate_matching(context, &execution_id, |resource| {
                if !matches!(resource.state, RuntimeResourceState::Running) {
                    return false;
                }
                if resource.authority.is_subset(&authority) {
                    return false;
                }
                resource.authority = resource
                    .authority
                    .intersection(&authority)
                    .cloned()
                    .collect();
                resource.state = RuntimeResourceState::Revoked {
                    reason: "execution authority narrowed below resource capability".into(),
                };
                true
            })?,
        }),
        JobCommand::Get { id } => {
            validate_id("runtime resource id", &id)?;
            Ok(JobResponse::Resource {
                resource: read(context, &id)?,
            })
        }
        JobCommand::List => Ok(JobResponse::Resources {
            resources: load_all(context)?,
        }),
    }
}

fn create(
    context: &JobContext<'_, '_>,
    id: String,
    kind: RuntimeResourceKind,
    owner_execution: String,
    authority: BTreeSet<String>,
) -> Result<RuntimeResourceRecord, String> {
    validate_id("runtime resource id", &id)?;
    validate_id("owner execution id", &owner_execution)?;
    let record = RuntimeResourceRecord {
        id,
        kind,
        owner_execution,
        promoted_to_workspace: false,
        authority,
        state: RuntimeResourceState::Running,
        output_references: Vec::new(),
    };
    insert(context, &record)?;
    Ok(record)
}

fn mutate_matching(
    context: &JobContext<'_, '_>,
    execution_id: &str,
    mut mutation: impl FnMut(&mut RuntimeResourceRecord) -> bool,
) -> Result<Vec<RuntimeResourceRecord>, String> {
    validate_id("execution id", execution_id)?;
    let mut affected = Vec::new();
    for mut resource in load_all(context)? {
        if resource.owner_execution != execution_id || !mutation(&mut resource) {
            continue;
        }
        replace(context, &resource)?;
        affected.push(resource);
    }
    affected.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(affected)
}

fn update(
    context: &JobContext<'_, '_>,
    id: &str,
    mutation: impl FnOnce(&mut RuntimeResourceRecord) -> Result<(), String>,
) -> Result<RuntimeResourceRecord, String> {
    validate_id("runtime resource id", id)?;
    let mut resource =
        read(context, id)?.ok_or_else(|| format!("unknown runtime resource: {id}"))?;
    mutation(&mut resource)?;
    replace(context, &resource)?;
    Ok(resource)
}

fn insert(context: &JobContext<'_, '_>, resource: &RuntimeResourceRecord) -> Result<(), String> {
    let key = resource_key(&resource.id);
    let old_index = read_raw(context, INDEX_KEY)?;
    let mut ids = decode_index(old_index.as_deref())?;
    if ids.iter().any(|id| id == &resource.id) || read_raw(context, &key)?.is_some() {
        return Err(format!("runtime resource already exists: {}", resource.id));
    }
    ids.push(resource.id.clone());
    ids.sort();
    context
        .kernel
        .transact_durable(
            &job_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: None,
                },
                TransactionOp::AssertValue {
                    key: INDEX_KEY.into(),
                    expected: old_index,
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(resource).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: INDEX_KEY.into(),
                    value: serde_json::to_vec(&ids).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn replace(context: &JobContext<'_, '_>, resource: &RuntimeResourceRecord) -> Result<(), String> {
    let key = resource_key(&resource.id);
    let old = read_raw(context, &key)?
        .ok_or_else(|| format!("unknown runtime resource: {}", resource.id))?;
    context
        .kernel
        .transact_durable(
            &job_namespace(),
            &[
                TransactionOp::AssertValue {
                    key: key.clone(),
                    expected: Some(old),
                },
                TransactionOp::Put {
                    key,
                    value: serde_json::to_vec(resource).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read(context: &JobContext<'_, '_>, id: &str) -> Result<Option<RuntimeResourceRecord>, String> {
    read_raw(context, &resource_key(id))?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn load_all(context: &JobContext<'_, '_>) -> Result<Vec<RuntimeResourceRecord>, String> {
    decode_index(read_raw(context, INDEX_KEY)?.as_deref())?
        .into_iter()
        .map(|id| read(context, &id)?.ok_or_else(|| format!("missing runtime resource: {id}")))
        .collect()
}

fn read_raw(context: &JobContext<'_, '_>, key: &str) -> Result<Option<Vec<u8>>, String> {
    context
        .kernel
        .read_durable(&job_namespace(), key)
        .map_err(|error| error.to_string())
}

fn decode_index(value: Option<&[u8]>) -> Result<Vec<String>, String> {
    value
        .map(|value| serde_json::from_slice(value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn resource_key(id: &str) -> String {
    format!("resource/{id}")
}

fn validate_id(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.contains('/') {
        Err(format!(
            "{label} must be non-empty and must not contain '/'"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_component_manifest;
    use phenix_core::{
        Kernel, LocalPersistence, PhenixValue, Project, ResolvedGeneration,
        ResolvedGenerationActivation,
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

    fn kernel(path: &PathBuf) -> Kernel {
        let manifest = job_manifest();
        let plugin = manifest.id.clone();
        let resolved = ResolvedGeneration::resolve_with_durable_schemas(
            [manifest],
            [job_component_manifest()],
            job_durable_schema_registrations(),
            [],
            &job_manifest().maximum_authority,
        )
        .unwrap();
        let persistence = LocalPersistence::open(path).unwrap();
        let mut kernel = Kernel::with_persistence(resolved.kernel_config().clone(), persistence);
        kernel.activate_resolved_generation(&resolved).unwrap();
        kernel
            .register_embedded_factory(plugin, job_factory)
            .unwrap();
        kernel.activate_all().unwrap();
        kernel
    }

    fn invoke(kernel: &mut Kernel, command: JobCommand) -> JobResponse {
        let component = job_component_manifest();
        let output = kernel
            .invoke_component(
                &component.id,
                &job_service(),
                &serde_json::to_vec(&PhenixValue::from(&command)).unwrap(),
                &job_manifest().maximum_authority,
                &component.owner,
            )
            .unwrap();
        let output: PhenixValue = serde_json::from_slice(&output).unwrap();
        JobResponse::try_from(Project(&output)).unwrap()
    }

    fn create_job(kernel: &mut Kernel, id: &str) {
        invoke(
            kernel,
            JobCommand::Create {
                id: id.into(),
                kind: RuntimeResourceKind::Job,
                owner_execution: "execution-1".into(),
                authority: BTreeSet::from(["workspace.read".into(), "workspace.write".into()]),
            },
        );
    }

    #[test]
    fn execution_termination_revokes_owned_resources_but_promoted_job_survives_restore() {
        let path = temp_db("jobs-lifetime");
        {
            let mut kernel = kernel(&path);
            create_job(&mut kernel, "job-revoked");
            create_job(&mut kernel, "job-promoted");
            invoke(
                &mut kernel,
                JobCommand::Promote {
                    id: "job-promoted".into(),
                },
            );
            let response = invoke(
                &mut kernel,
                JobCommand::ExecutionTerminated {
                    execution_id: "execution-1".into(),
                },
            );
            match response {
                JobResponse::Affected { resources } => assert_eq!(resources.len(), 1),
                other => panic!("unexpected response: {other:?}"),
            }
        }
        let mut restored = kernel(&path);
        let revoked = invoke(
            &mut restored,
            JobCommand::Get {
                id: "job-revoked".into(),
            },
        );
        let promoted = invoke(
            &mut restored,
            JobCommand::Get {
                id: "job-promoted".into(),
            },
        );
        match revoked {
            JobResponse::Resource {
                resource: Some(resource),
            } => {
                assert!(matches!(
                    resource.state,
                    RuntimeResourceState::Revoked { .. }
                ));
            }
            other => panic!("unexpected response: {other:?}"),
        }
        match promoted {
            JobResponse::Resource {
                resource: Some(resource),
            } => {
                assert!(resource.promoted_to_workspace);
                assert_eq!(resource.state, RuntimeResourceState::Running);
            }
            other => panic!("unexpected response: {other:?}"),
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn authority_narrowing_revokes_incompatible_resource() {
        let path = temp_db("jobs-authority");
        let mut kernel = kernel(&path);
        create_job(&mut kernel, "job-1");
        let response = invoke(
            &mut kernel,
            JobCommand::NarrowAuthority {
                execution_id: "execution-1".into(),
                authority: BTreeSet::from(["workspace.read".into()]),
            },
        );
        match response {
            JobResponse::Affected { resources } => {
                assert_eq!(resources.len(), 1);
                assert_eq!(
                    resources[0].authority,
                    BTreeSet::from(["workspace.read".into()])
                );
                assert!(matches!(
                    resources[0].state,
                    RuntimeResourceState::Revoked { .. }
                ));
            }
            other => panic!("unexpected response: {other:?}"),
        }
        let _ = fs::remove_file(path);
    }
}
