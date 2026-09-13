use crate::model_routing_manifest;
use phenix_core::{
    Authority, CapabilityId, ComponentExport, ComponentId, ComponentInterface, ComponentManifest,
    PluginId,
};
use phenix_sdk::{ModelDispatchInterface, ModelRoutingInterface};

const MODEL_ROUTING_COMPONENT: &str = "phenix.models";
const MODEL_ROUTING_PLUGIN: &str = "phenix.models";
const PERSISTENCE_SCHEMA: &str = "kernel.persistence.schema";
const PERSISTENCE_READ: &str = "kernel.persistence.read";
const PERSISTENCE_WRITE: &str = "kernel.persistence.write";

#[must_use]
pub fn model_routing_component_id() -> ComponentId {
    ComponentId::parse(MODEL_ROUTING_COMPONENT).expect("static model routing component id is valid")
}

#[must_use]
pub fn model_routing_component_manifest(maximum_authority: Authority) -> ComponentManifest {
    let authority = model_routing_manifest(maximum_authority).maximum_authority;
    ComponentManifest {
        listeners: Vec::new(),
        id: model_routing_component_id(),
        owner: PluginId::parse(MODEL_ROUTING_PLUGIN)
            .expect("static model routing plugin id is valid"),
        imports: Vec::new(),
        exports: [
            (ModelRoutingInterface::interface_id(), ModelRoutingInterface::schema()),
            (ModelDispatchInterface::interface_id(), ModelDispatchInterface::schema()),
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

fn persistence_authority() -> Authority {
    Authority::new([
        CapabilityId::parse(PERSISTENCE_SCHEMA).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_READ).expect("static capability is valid"),
        CapabilityId::parse(PERSISTENCE_WRITE).expect("static capability is valid"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{
        ComponentId, ComponentImport, PluginExecution, PluginManifest, ResolvedComponentGraph,
    };

    fn plugin(value: &str) -> PluginId {
        PluginId::parse(value).unwrap()
    }

    fn component(value: &str) -> ComponentId {
        ComponentId::parse(value).unwrap()
    }

    fn consumer_manifest(authority: Authority) -> PluginManifest {
        PluginManifest {
            id: plugin("fixture.model-consumer"),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: authority,
        }
    }

    fn consumer_component(authority: Authority) -> ComponentManifest {
        ComponentManifest {
            listeners: Vec::new(),
            id: component("fixture.model-consumer"),
            owner: plugin("fixture.model-consumer"),
            imports: vec![
                ComponentImport {
                    interface: ModelRoutingInterface::interface_id(),
                    schema: ModelRoutingInterface::schema(),
                    required: true,
                    authority: authority.clone(),
                },
                ComponentImport {
                    interface: ModelDispatchInterface::interface_id(),
                    schema: ModelDispatchInterface::schema(),
                    required: true,
                    authority: authority.clone(),
                },
            ],
            exports: Vec::new(),
            maximum_authority: authority,
        }
    }

    #[test]
    fn first_party_model_exports_routing_and_exact_dispatch() {
        let manifest = model_routing_component_manifest(Authority::default());
        assert_eq!(manifest.id, model_routing_component_id());
        assert_eq!(manifest.owner.as_str(), MODEL_ROUTING_PLUGIN);
        assert_eq!(manifest.exports.len(), 2);
        assert_eq!(
            manifest.exports[0].interface,
            ModelRoutingInterface::interface_id()
        );
        assert_eq!(
            manifest.exports[1].interface,
            ModelDispatchInterface::interface_id()
        );
        assert!(manifest
            .exports
            .iter()
            .all(|export| export.required_authority == persistence_authority()));
    }

    #[test]
    fn model_interfaces_bind_without_package_only_authority() {
        let network = CapabilityId::parse("network.openai").unwrap();
        let package_authority = Authority::new(
            persistence_authority()
                .capabilities()
                .cloned()
                .chain([network]),
        );
        let consumer_authority = persistence_authority();
        let graph = ResolvedComponentGraph::compile(
            [
                consumer_manifest(consumer_authority.clone()),
                model_routing_manifest(package_authority.clone()),
            ],
            [
                consumer_component(consumer_authority.clone()),
                model_routing_component_manifest(package_authority),
            ],
            &consumer_authority,
        )
        .unwrap();

        for interface in [
            ModelRoutingInterface::interface_id(),
            ModelDispatchInterface::interface_id(),
        ] {
            assert!(graph
                .import_handle(&component("fixture.model-consumer"), &interface)
                .unwrap()
                .is_some());
        }
    }
}
