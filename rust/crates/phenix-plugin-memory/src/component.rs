use crate::{implementation::persistence_authority, memory_manifest};
use phenix_core::{
    ComponentExport, ComponentId, ComponentImport, ComponentInterface, ComponentManifest, PluginId,
};
use phenix_sdk::{
    ContextCompactionInterface, ContextExpansionInterface, HelperInvocationInterface,
    MemoryContextInterface, MemoryEmbeddingInterface, MemoryInterface, MemoryRankInterface,
    OptionsInterface,
};

const MEMORY_COMPONENT: &str = "phenix.memory";
const MEMORY_PLUGIN: &str = "phenix.memory";

#[must_use]
pub fn memory_component_id() -> ComponentId {
    ComponentId::parse(MEMORY_COMPONENT).expect("static component id is valid")
}

#[must_use]
pub fn memory_component_manifest() -> ComponentManifest {
    let authority = memory_manifest().maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: memory_component_id(),
        owner: PluginId::parse(MEMORY_PLUGIN).expect("static plugin id is valid"),
        imports: vec![
            ComponentImport {
                interface: HelperInvocationInterface::interface_id(),
                schema: HelperInvocationInterface::schema(),
                required: true,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: OptionsInterface::interface_id(),
                schema: OptionsInterface::schema(),
                required: false,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: MemoryEmbeddingInterface::interface_id(),
                schema: MemoryEmbeddingInterface::schema(),
                required: false,
                authority: authority.clone(),
            },
            ComponentImport {
                interface: MemoryRankInterface::interface_id(),
                schema: MemoryRankInterface::schema(),
                required: false,
                authority: authority.clone(),
            },
        ],
        exports: [
            (MemoryInterface::interface_id(), MemoryInterface::schema()),
            (
                MemoryContextInterface::interface_id(),
                MemoryContextInterface::schema(),
            ),
            (
                ContextCompactionInterface::interface_id(),
                ContextCompactionInterface::schema(),
            ),
            (
                ContextExpansionInterface::interface_id(),
                ContextExpansionInterface::schema(),
            ),
        ]
        .into_iter()
        .map(|(interface, schema)| ComponentExport {
            interface,
            schema,
            priority: 100,
            required_authority: persistence_authority(),
        })
        .collect(),
        maximum_authority: authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_manifest_exports_each_memory_semantic_batch() {
        let manifest = memory_component_manifest();
        assert_eq!(manifest.id, memory_component_id());
        assert_eq!(manifest.owner.as_str(), MEMORY_PLUGIN);
        assert_eq!(manifest.imports.len(), 4);
        assert_eq!(
            manifest.imports[0].interface,
            HelperInvocationInterface::interface_id()
        );
        assert!(manifest.imports[0].required);
        assert_eq!(
            manifest.imports[1].interface,
            OptionsInterface::interface_id()
        );
        assert!(!manifest.imports[1].required);
        assert_eq!(
            manifest.imports[2].interface,
            MemoryEmbeddingInterface::interface_id()
        );
        assert!(!manifest.imports[2].required);
        assert_eq!(
            manifest.imports[3].interface,
            MemoryRankInterface::interface_id()
        );
        assert!(!manifest.imports[3].required);
        assert_eq!(manifest.exports.len(), 4);
        for interface in [
            MemoryInterface::interface_id(),
            MemoryContextInterface::interface_id(),
            ContextCompactionInterface::interface_id(),
            ContextExpansionInterface::interface_id(),
        ] {
            assert!(manifest
                .exports
                .iter()
                .any(|export| export.interface == interface));
        }
        assert!(manifest
            .exports
            .iter()
            .all(|export| export.required_authority == persistence_authority()));
    }

    #[test]
    fn higher_priority_memory_provider_replaces_default_without_kernel_changes() {
        let alternate_owner = PluginId::parse("fixture.memory").unwrap();
        let consumer_owner = PluginId::parse("fixture.consumer").unwrap();

        let mut alternate_plugin = memory_manifest();
        alternate_plugin.id = alternate_owner.clone();
        alternate_plugin.services.clear();
        alternate_plugin.resource_namespaces.clear();

        let mut consumer_plugin = memory_manifest();
        consumer_plugin.id = consumer_owner.clone();
        consumer_plugin.services.clear();
        consumer_plugin.resource_namespaces.clear();

        let alternate_component = ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.memory").unwrap(),
            owner: alternate_owner,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: MemoryInterface::interface_id(),
                schema: MemoryInterface::schema(),
                priority: 200,
                required_authority: persistence_authority(),
            }],
            maximum_authority: persistence_authority(),
        };
        let consumer_component = ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.consumer").unwrap(),
            owner: consumer_owner,
            imports: vec![ComponentImport {
                interface: MemoryInterface::interface_id(),
                schema: MemoryInterface::schema(),
                required: true,
                authority: persistence_authority(),
            }],
            exports: Vec::new(),
            maximum_authority: persistence_authority(),
        };
        let first_party_component = ComponentManifest {
            listeners: Vec::new(),
            id: memory_component_id(),
            owner: memory_manifest().id,
            imports: Vec::new(),
            exports: vec![ComponentExport {
                interface: MemoryInterface::interface_id(),
                schema: MemoryInterface::schema(),
                priority: 100,
                required_authority: persistence_authority(),
            }],
            maximum_authority: persistence_authority(),
        };

        let graph = phenix_core::ResolvedComponentGraph::compile(
            [memory_manifest(), alternate_plugin, consumer_plugin],
            [
                first_party_component,
                alternate_component.clone(),
                consumer_component.clone(),
            ],
            &persistence_authority(),
        )
        .unwrap();
        let handle = graph
            .import_handle(&consumer_component.id, &MemoryInterface::interface_id())
            .unwrap()
            .unwrap();
        assert_eq!(handle.exporter(), &alternate_component.id);
    }
}
