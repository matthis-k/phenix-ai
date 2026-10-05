use phenix_core::{
    Authority, CONTEXT_SERVICE, ComponentId, ComponentInterface, ComponentManifest, ContextCommand,
    ContextDescriptor, ContextResourceId, ContextResourceRevision, ContextResponse,
    ContextRevisionId, InterfaceId, PermissionId, PluginContext, PluginInstance, PluginManifest,
    ResourceNamespace, TransactionOp,
};
use phenix_sdk::StaticPluginDefinition;
use sha2::{Digest, Sha256};

pub const BASIC_CONTEXT_PLUGIN: &str = "phenix.basic-context";
pub const BASIC_CONTEXT_COMPONENT: &str = "phenix.basic-context";
const BASIC_CONTEXT_NAMESPACE: &str = "phenix.basic-context.state";
const INDEX_KEY: &str = "context/@all";

type BasicContextContext<'host, 'runtime> = PluginContext<'host, 'runtime, ()>;

pub struct BasicContextInterface;

impl ComponentInterface for BasicContextInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(CONTEXT_SERVICE).expect("static context interface id is valid")
    }
}

struct ContextStore;

#[phenix_sdk::resource(schema = 1)]
impl ContextStore {}

#[phenix_sdk::plugin(root, id = "phenix.basic-context", authority = persistence_authority())]
pub struct Plugin {
    #[phenix(resource, id = "phenix.basic-context.state")]
    _state: phenix_sdk::Durable<ContextStore>,
}

#[phenix_sdk::plugin]
impl Plugin {
    #[phenix(export("phenix.context@1"), terminal, priority = 10)]
    fn handle(
        &self,
        context: &phenix_sdk::PluginContext<'_, '_, ()>,
        command: ContextCommand,
    ) -> Result<ContextResponse, String> {
        handle(context, command)
    }
}

#[must_use]
pub fn basic_context_manifest() -> PluginManifest {
    Plugin::manifest()
}

#[must_use]
pub fn basic_context_component_manifest() -> ComponentManifest {
    Plugin::component_manifests()
        .into_iter()
        .next()
        .expect("basic context plugin has one generated component")
}

#[must_use]
pub fn basic_context_factory() -> Box<dyn PluginInstance> {
    Plugin {
        _state: phenix_sdk::Durable::new(),
    }
    .__phenix_into_plugin_instance()
}

#[must_use]
pub fn basic_context_component_id() -> ComponentId {
    basic_context_component_manifest().id
}

fn handle(
    context: &BasicContextContext<'_, '_>,
    command: ContextCommand,
) -> Result<ContextResponse, String> {
    match command {
        ContextCommand::Register {
            resource_id,
            kind,
            source,
            scope,
            content,
        } => {
            let content_identity = content_identity(content.as_ref());
            let revision =
                ContextRevisionId::parse(content_identity.clone()).map_err(str::to_owned)?;
            let resource = ContextResourceRevision {
                descriptor: ContextDescriptor {
                    resource_id: resource_id.clone(),
                    revision: revision.clone(),
                    kind,
                    source,
                    scope,
                    content_identity,
                    estimated_bytes: u64::try_from(content.as_ref().len())
                        .map_err(|_| "context resource byte length exceeds u64".to_owned())?,
                },
                content,
            };
            write_resource(context, &resource)?;
            Ok(ContextResponse::Registered { resource })
        }
        ContextCommand::Get {
            resource_id,
            revision,
        } => Ok(ContextResponse::Resource {
            resource: read_resource(context, &resource_id, &revision)?,
        }),
        ContextCommand::List => Ok(ContextResponse::Resources {
            descriptors: read_index(context)?
                .into_iter()
                .map(|(id, revision)| {
                    read_resource(context, &id, &revision)?
                        .map(|resource| resource.descriptor)
                        .ok_or_else(|| format!("missing durable context revision: {id}@{revision}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        }),
    }
}

fn write_resource(
    context: &BasicContextContext<'_, '_>,
    resource: &ContextResourceRevision,
) -> Result<(), String> {
    let identity = (
        resource.descriptor.resource_id.clone(),
        resource.descriptor.revision.clone(),
    );
    let mut index = read_index(context)?;
    if !index.contains(&identity) {
        index.push(identity.clone());
        index.sort();
    }
    context
        .kernel
        .transact_durable(
            &namespace(),
            &[
                TransactionOp::Put {
                    key: resource_key(&identity.0, &identity.1),
                    value: serde_json::to_vec(resource).map_err(|error| error.to_string())?,
                },
                TransactionOp::Put {
                    key: INDEX_KEY.into(),
                    value: serde_json::to_vec(&index).map_err(|error| error.to_string())?,
                },
            ],
        )
        .map_err(|error| error.to_string())
}

fn read_resource(
    context: &BasicContextContext<'_, '_>,
    id: &ContextResourceId,
    revision: &ContextRevisionId,
) -> Result<Option<ContextResourceRevision>, String> {
    context
        .kernel
        .read_durable(&namespace(), &resource_key(id, revision))
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .transpose()
}

fn read_index(
    context: &BasicContextContext<'_, '_>,
) -> Result<Vec<(ContextResourceId, ContextRevisionId)>, String> {
    context
        .kernel
        .read_durable(&namespace(), INDEX_KEY)
        .map_err(|error| error.to_string())?
        .map(|value| serde_json::from_slice(&value).map_err(|error| error.to_string()))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn resource_key(id: &ContextResourceId, revision: &ContextRevisionId) -> String {
    format!("resource/{id}/{revision}")
}

fn content_identity(content: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(content))
}

fn namespace() -> ResourceNamespace {
    ResourceNamespace::parse(BASIC_CONTEXT_NAMESPACE).expect("static namespace is valid")
}

fn persistence_authority() -> Authority {
    Authority::new([
        capability("kernel.persistence.schema"),
        capability("kernel.persistence.read"),
        capability("kernel.persistence.write"),
    ])
}

fn capability(value: &str) -> PermissionId {
    PermissionId::parse(value).expect("static capability is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_authoring_preserves_stable_identity() {
        let manifest = basic_context_manifest();
        assert_eq!(manifest.id.as_str(), BASIC_CONTEXT_PLUGIN);
        assert!(manifest.services.is_empty());
        assert_eq!(manifest.resource_namespaces, vec![namespace()]);

        let component = basic_context_component_manifest();
        assert_eq!(component.id.as_str(), BASIC_CONTEXT_COMPONENT);
        assert_eq!(component.exports.len(), 1);
        assert_eq!(
            component.exports[0].interface,
            BasicContextInterface::interface_id()
        );
    }
}
