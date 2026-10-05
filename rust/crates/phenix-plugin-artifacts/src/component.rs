use crate::{ARTIFACT_SERVICE, Plugin};
use phenix_core::{ComponentId, ComponentInterface, ComponentManifest, InterfaceId};
use phenix_sdk::StaticPluginDefinition;

pub struct ArtifactInterface;

impl ComponentInterface for ArtifactInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(ARTIFACT_SERVICE).expect("static artifact interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::fallible_of::<
            crate::ArtifactCommand,
            crate::ArtifactResponse,
            String,
        >()
    }
}

#[must_use]
pub fn artifact_component_id() -> ComponentId {
    artifact_component_manifest().id
}

#[must_use]
pub fn artifact_component_manifest() -> ComponentManifest {
    Plugin::component_manifests()
        .into_iter()
        .next()
        .expect("artifacts plugin has one generated component")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_manifest;
    use phenix_core::{
        ComponentImport, PluginExecution, PluginId, PluginManifest, ResolvedComponentGraph,
    };

    #[test]
    fn artifact_service_binds_as_an_ordinary_typed_component() {
        let consumer_plugin = PluginManifest {
            id: PluginId::parse("fixture.artifact-consumer").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: phenix_core::Authority::default(),
        };
        let consumer = ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.artifact-consumer").unwrap(),
            owner: consumer_plugin.id.clone(),
            imports: vec![ComponentImport {
                interface: ArtifactInterface::interface_id(),
                schema: ArtifactInterface::schema(),
                required: true,
                authority: phenix_core::Authority::default(),
            }],
            exports: Vec::new(),
            maximum_authority: phenix_core::Authority::default(),
        };
        let graph = ResolvedComponentGraph::compile(
            [consumer_plugin, artifact_manifest()],
            [consumer, artifact_component_manifest()],
            &artifact_manifest().maximum_authority,
        )
        .unwrap();

        let handle = graph
            .import_handle(
                &ComponentId::parse("fixture.artifact-consumer").unwrap(),
                &ArtifactInterface::interface_id(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(handle.exporter(), &artifact_component_id());
    }
}
