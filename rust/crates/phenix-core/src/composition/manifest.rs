use crate::{
    ArtifactRevision, Authority, ComponentId, EventFailurePolicy, EventTypeId, InterfaceId,
    InterfaceSchema, PhenixSchema, PhenixValue, PluginId, ResourceNamespace, RuntimeId, ServiceId,
    SubscriptionId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PluginArtifact {
    pub locator: String,
    pub revision: ArtifactRevision,
    #[serde(default)]
    pub configuration: BTreeMap<String, PhenixValue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginExecution<A = PluginArtifact> {
    Embedded,
    Runtime { runtime: RuntimeId, artifact: A },
    ResourceOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceRole {
    Terminal,
    Layer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServiceContribution {
    pub service: ServiceId,
    pub role: ServiceRole,
    pub priority: i32,
    pub required_authority: Authority,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentImport {
    pub interface: InterfaceId,
    #[serde(default)]
    pub schema: InterfaceSchema,
    pub required: bool,
    pub authority: Authority,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentExport {
    pub interface: InterfaceId,
    #[serde(default)]
    pub schema: InterfaceSchema,
    pub priority: i32,
    pub required_authority: Authority,
}

/// Declarative listener entry owned by a component's resolved generation.
///
/// This is topology only: which event the component observes, which function
/// handles it, the structural payload contract, and the authority required to
/// deliver the event. The live handler binding is resolved from this entry
/// during activation; it is not part of the declaration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerProjection {
    Project,
    Exact,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentListener {
    pub id: SubscriptionId,
    pub event: EventTypeId,
    pub event_version: u32,
    pub method: String,
    #[serde(default = "any_schema")]
    pub payload_schema: PhenixSchema,
    pub projection: ListenerProjection,
    #[serde(default)]
    pub dependencies: Vec<SubscriptionId>,
    pub failure_policy: EventFailurePolicy,
    #[serde(default)]
    pub required_authority: Authority,
}

fn any_schema() -> PhenixSchema {
    PhenixSchema::Any
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentManifest {
    pub id: ComponentId,
    pub owner: PluginId,
    pub imports: Vec<ComponentImport>,
    pub exports: Vec<ComponentExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listeners: Vec<ComponentListener>,
    pub maximum_authority: Authority,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PluginManifest<A = PluginArtifact> {
    pub id: PluginId,
    pub version: u32,
    pub execution: PluginExecution<A>,
    pub dependencies: Vec<PluginId>,
    pub services: Vec<ServiceContribution>,
    pub resource_namespaces: Vec<ResourceNamespace>,
    pub maximum_authority: Authority,
}

impl<A> PluginManifest<A> {
    pub fn map_artifact<B>(self, map: impl FnOnce(A) -> B) -> PluginManifest<B> {
        let execution = match self.execution {
            PluginExecution::Embedded => PluginExecution::Embedded,
            PluginExecution::Runtime { runtime, artifact } => PluginExecution::Runtime {
                runtime,
                artifact: map(artifact),
            },
            PluginExecution::ResourceOnly => PluginExecution::ResourceOnly,
        };
        PluginManifest {
            id: self.id,
            version: self.version,
            execution,
            dependencies: self.dependencies,
            services: self.services,
            resource_namespaces: self.resource_namespaces,
            maximum_authority: self.maximum_authority,
        }
    }
}

impl PluginManifest {
    pub fn resource_only(id: PluginId) -> Self {
        Self {
            id,
            version: 1,
            execution: PluginExecution::ResourceOnly,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_metadata_is_inspectable_without_plugin_activation() {
        let manifest = PluginManifest {
            id: PluginId::parse("third-party").unwrap(),
            version: 7,
            execution: PluginExecution::Runtime {
                runtime: RuntimeId::parse("vendor.runtime").unwrap(),
                artifact: PluginArtifact {
                    locator: "plugin.wasm".into(),
                    revision: ArtifactRevision::from_content(b"fixture"),
                    configuration: BTreeMap::from([(
                        "entrypoint".into(),
                        PhenixValue::String("start".into()),
                    )]),
                },
            },
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let encoded = serde_json::to_value(&manifest).unwrap();

        assert_eq!(encoded["id"], "third-party");
        assert_eq!(encoded["version"], 7);
        assert_eq!(encoded["execution"]["kind"], "runtime");
        assert_eq!(encoded["execution"]["runtime"], "vendor.runtime");
        assert_eq!(encoded["execution"]["artifact"]["locator"], "plugin.wasm");
        assert_eq!(
            encoded["execution"]["artifact"]["revision"],
            ArtifactRevision::from_content(b"fixture").as_ref()
        );
    }
}
