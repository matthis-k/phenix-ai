use crate::{REPOSITORY_WORK_QUEUE_SERVICE, repository_worker_manifest};
use phenix_core::{
    Authority, ComponentExport, ComponentId, ComponentInterface, ComponentManifest, InterfaceId,
    PluginId,
};

const REPOSITORY_WORKER_COMPONENT: &str = "phenix.repository-workers";
const REPOSITORY_WORKER_PLUGIN: &str = "phenix.repository-workers";

pub struct RepositoryWorkerInterface;

impl ComponentInterface for RepositoryWorkerInterface {
    fn interface_id() -> InterfaceId {
        InterfaceId::parse(REPOSITORY_WORK_QUEUE_SERVICE)
            .expect("static repository worker interface id is valid")
    }

    fn schema() -> phenix_core::InterfaceSchema {
        phenix_core::InterfaceSchema::of::<
            crate::RepositoryWorkSnapshot,
            crate::service::RepositoryWorkQueueResponse,
        >()
    }
}

#[must_use]
pub fn repository_worker_component_id() -> ComponentId {
    ComponentId::parse(REPOSITORY_WORKER_COMPONENT)
        .expect("static repository worker component id is valid")
}

#[must_use]
pub fn repository_worker_component_manifest() -> ComponentManifest {
    ComponentManifest {
        listeners: Vec::new(),
        id: repository_worker_component_id(),
        owner: PluginId::parse(REPOSITORY_WORKER_PLUGIN)
            .expect("static repository worker plugin id is valid"),
        imports: Vec::new(),
        exports: vec![ComponentExport {
            interface: RepositoryWorkerInterface::interface_id(),
            schema: RepositoryWorkerInterface::schema(),
            priority: 100,
            required_authority: Authority::default(),
        }],
        maximum_authority: repository_worker_manifest().maximum_authority,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phenix_core::{ComponentImport, PluginExecution, PluginManifest, ResolvedComponentGraph};

    #[test]
    fn repository_worker_queue_binds_as_an_ordinary_typed_component() {
        let consumer_plugin = PluginManifest {
            id: PluginId::parse("fixture.repository-worker-consumer").unwrap(),
            version: 1,
            execution: PluginExecution::Embedded,
            dependencies: Vec::new(),
            services: Vec::new(),
            resource_namespaces: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let consumer = ComponentManifest {
            listeners: Vec::new(),
            id: ComponentId::parse("fixture.repository-worker-consumer").unwrap(),
            owner: consumer_plugin.id.clone(),
            imports: vec![ComponentImport {
                interface: RepositoryWorkerInterface::interface_id(),
                schema: RepositoryWorkerInterface::schema(),
                required: true,
                authority: Authority::default(),
            }],
            exports: Vec::new(),
            maximum_authority: Authority::default(),
        };
        let graph = ResolvedComponentGraph::compile(
            [consumer_plugin, repository_worker_manifest()],
            [consumer, repository_worker_component_manifest()],
            &Authority::default(),
        )
        .unwrap();

        let handle = graph
            .import_handle(
                &ComponentId::parse("fixture.repository-worker-consumer").unwrap(),
                &RepositoryWorkerInterface::interface_id(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(handle.exporter(), &repository_worker_component_id());
    }
}
