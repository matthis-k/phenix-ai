use crate::workspace_manifest;
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentImport, ComponentInterface,
    ComponentManifest, PluginId,
};
use phenix_plugin_artifacts::ArtifactInterface;
use phenix_sdk::WorkspaceInterface;

const WORKSPACE_COMPONENT: &str = "phenix.workspace";
const WORKSPACE_PLUGIN: &str = "phenix.workspace";

#[must_use]
pub fn workspace_component_id() -> ComponentId {
    ComponentId::parse(WORKSPACE_COMPONENT).expect("static workspace component id is valid")
}

#[must_use]
pub fn workspace_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: workspace_component_id(),
        owner: PluginId::parse(WORKSPACE_PLUGIN).expect("static workspace plugin id is valid"),
        imports: vec![ComponentImport {
            interface: ArtifactInterface::interface_id(),
            schema: ArtifactInterface::schema(),
            required: false,
            authority: Authority::default(),
        }],
        exports: vec![ComponentExport {
            interface: WorkspaceInterface::interface_id(),
            schema: WorkspaceInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: workspace_manifest().maximum_authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        CapabilityId, ComponentImport, PluginExecution, PluginManifest, ResolvedComponentGraph,
    };

    fn capability(value: &str) -> CapabilityId {
        CapabilityId::parse(value).unwrap()
    }

    fn consumer_manifest(maximum_authority: Authority) -> PluginManifest {
        PluginManifest {
            id: PluginId::parse("fixture.workspace-consumer").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority,
        }
    }

    fn consumer_component(authority: Authority) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.workspace-consumer").unwrap(),
            owner: PluginId::parse("fixture.workspace-consumer").unwrap(),
            imports: vec![ComponentImport {
                interface: WorkspaceInterface::interface_id(),
                schema: WorkspaceInterface::schema(),
                required: true,
                authority: authority.clone(),
            }],
            exports: Vec::new(),
            maximum_authority: authority,
        }
    }

    #[test]
    fn workspace_typed_binding_attenuates_to_consumer_authority() {
        let read = capability("workspace.read");
        let write = capability("workspace.write");
        let consumer_authority = Authority::new([read.clone()]);
        let graph = ResolvedComponentGraph::compile(
            [
                consumer_manifest(consumer_authority.clone()),
                workspace_manifest(),
            ],
            [
                consumer_component(consumer_authority.clone()),
                workspace_component_manifest(),
            ],
            &workspace_manifest().maximum_authority,
        )
        .unwrap();

        let handle = graph
            .import_handle(
                &ComponentId::parse("fixture.workspace-consumer").unwrap(),
                &WorkspaceInterface::interface_id(),
            )
            .unwrap()
            .unwrap();
        assert!(handle.effective_authority().permits(&read));
        assert!(!handle.effective_authority().permits(&write));
        assert_eq!(handle.exporter(), &workspace_component_id());
    }
}
